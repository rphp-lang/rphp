# PHP heap (`rphp::heap`): návrh a stav

Větev `codex/php-heap-v1` (základ main `3e1f2580`). Cíl zadaný uživatelem:
vlastní alokátor přizpůsobený PHP hodnotám, který výkonově překoná obecné
alokátory (glibc, mimalloc) na skutečných PHP programech, s maximem práce
v rychlé cestě a s ASM tam, kde prokazatelně pomůže. Alokátor je pro celý
runtime (CLI, dlouhé běhy, více vláken), nejen pro PHPStan.

## Měřená východiska (cold PHPStan, 24,6 M alokací)

- 99 % bloků ≤ 1 KiB, 69 % ≤ 48 B, 96 % ≤ 192 B; zarovnání ≤ 16.
- 59 % bloků zanikne do 4 dalších alokací → LIFO znovupoužití drží blok v L1.
- Nejčastější velikosti: 40 B (`RcBox<String>`), 24, 48 (`Vec<Value>` ×3),
  152 (`RcBox<PhpArray>`), 1–24 B bytové buffery řetězců, 84, 104, 128.
- Cross-thread free 2,5 % (parser vlákno alokuje AST, hlavní vlákno uvolňuje).
- glibc malloc/free = 16 % instrukcí a ~20 % času cold běhu.

## Architektura v7 (aktuální stav větve)

- **Rezervace adresního prostoru**: jeden `mmap(MAP_NORESERVE)` 64 GiB
  (fallback poloviční až 256 MiB) s `madvise(MADV_HUGEPAGE)` na celý rozsah
  (`RPHP_HEAP_THP=0` vypne). Stránky se odřezávají souvisle, takže první
  dotyk faultuje po 2 MiB místo 4 KiB a TLB pokryje 512× více heapu na
  záznam; THP je v systému v režimu `madvise`, glibc ani mimalloc ho tedy
  nedostanou. Dolní polovina = region malých stránek (64 KiB), třetí
  čtvrtina = region středních stránek (1 MiB), poslední čtvrtina = region
  velkých bloků. Vlastnictví bloku = jedno odečtení a porovnání s rozsahem
  (`pool_offset`), hlavička stránky = maska podle regionu; žádné hlavičky
  bloků u malých a středních tříd. Rozsahové hodnoty leží v jedné 64 B
  statice (`RANGE`), takže rychlá cesta bez LTO dělá jediný GOT load.
- **Velikostní třídy** (40): malé 8, 16, 24, 32, 40, 48, 56, 64, 80, 96,
  112, 128, 152, 168, 192, 256, 384, 512, 768, 1024 (dvě konstantní tabulky
  `(size+7)/8 → třída`, pro align ≤ 8 a pro align 16); střední 1280 … 32768
  ve čtyřech krocích na oktávu (1,25×, 1,5×, 1,75×, 2×), třída spočtená
  z `bsr`. **Velké bloky** 32 KiB–4 MiB: násobky 64 KiB s 128 B hlavičkou
  (počet jednotek), globální lock-free zásobník volných bloků na každý
  počet jednotek (64 kbelíků), nad 256 MiB volných bajtů `MADV_DONTNEED`
  těla bloku; realloc v rámci zaokrouhlené kapacity vrací tentýž blok. Nad
  4 MiB nebo align > 16 → systémový alokátor (bloky leží mimo rezervaci).
  Důvod: glibc obsluhovalo bloky > 32 KiB přes `brk` heap s trimem, což
  stálo ~70 k opakovaných 4 KiB faultů na cold běh PHPStanu.
- **Stránka** = jedna třída; hlavička 128 B, horké owner-only položky
  v prvním řádku cache: `local_free` (LIFO uvolněných bloků vlastníka),
  `bump`, `end`, `used` (počet vydaných bloků), `class`; dále
  `owner_state` (adresa heapu vlastníka | příznak QUEUED | počet
  právě zařazujících remote uvolňovatelů), atomický `remote_free` list,
  `next_queued` (fronta remote stránek u vlastníka nebo globální pool
  volných stránek), `listing` (0 nezařazena / 1 v seznamu dostupných / 2
  aktuální stránka třídy), oboustranně vázané seznamy všech stránek vlastníka
  a dostupných stránek třídy.
- **Thread heap** (`thread_local!` bez destruktoru, 64 B zarovnaný): per
  třída aktuální stránka a seznam dalších dostupných stránek, seznam všech
  stránek, `mode`, atomická fronta stránek s remote free.
- **Rychlá cesta alloc**: `class_for` → aktuální stránka třídy → pop
  z `local_free` stránky, jinak bump, `used += 1`; při vyčerpání tail-call
  do studeného `alloc_slow` (rozhodnutí režimu při prvním použití → `refill`:
  drain fronty remote stránek → aktuální stránka → další dostupná stránka →
  adopce osiřelé → recyklovaná z globálního poolu → nová z rezervace →
  systémový fallback). Režim se v horké cestě nečte: v režimu system jsou
  všechny aktuální stránky null.
- **Rychlá cesta free**: offset v rezervaci → hlavička → `owner_state`
  patří našemu heapu → push na `local_free` stránky, `used -= 1`; stránka,
  která tím poprvé získala volný blok, se zařadí mezi dostupné; stránka,
  která se vyprázdnila a není aktuální, jde do globálního poolu (`recycle_page`).
  Cizí blok → CAS push na `remote_free` stránky a (není-li už QUEUED)
  zařazení stránky do fronty vlastníka pod ochranou počtu pusherů.
- **Protokol vlastnictví** (řeší závod remote free × zánik vlastníka):
  remote uvolňovatel CAS-em nastaví QUEUED a zvýší počet pusherů, teprve
  pak sáhne na heap vlastníka a poté počet sníží. Vlastník před vzdáním se
  stránky (`detach`/`try_detach`) čeká na nulový počet pusherů a CAS-em
  nuluje stav; `try_detach` odmítne, dokud je stránka QUEUED, aby ve frontě
  nezůstal záznam na stránku, kterou už nevlastní.
- **Recyklace stránek**: plně volná neaktuální stránka → per-region
  lock-free pool volných stránek (znovu použitelný libovolnou třídou,
  hlavička se přepíše). Nad rozpočet 64 MiB rezidentních stránek na region
  se blokový prostor stránky vrací OS přes `madvise(MADV_DONTNEED)`
  (částečný DONTNEED rozbije THP stránku; rozpočet 16 MiB měřitelně nic
  nezlepšil ani nezhoršil, 64 MiB je rezerva pro fázové workloady).
- **Zánik vlákna**: `EXIT_GUARD` (TLS s destruktorem, registrovaný při
  první stránce) přesune plně volné stránky do poolu a ostatní do orphan
  seznamu (mutex, přeskočen při `ORPHAN_COUNT == 0`), přepne vlákno do
  režimu system (pozdější TLS destruktory alokují u systému, nic neuniká).
- **realloc**: ve stejné třídě vrací tentýž blok; jinak alloc+copy+free.
  Systémový blok zůstává u systému.
- **Debug režim** (`--features php-heap-debug`): uvolněný blok otráven
  0xDE, druhé slovo nese 64-bit značku; opakované uvolnění téhož bloku →
  abort s popisem bloku a stavu stránky (třída, `used`, listing, zda je
  v lokálním/remote listu). Dále: blok vydaný z free listu musí značku
  nést (jinak zápis po uvolnění), `dealloc`/`realloc` musí dostat layout
  odpovídající třídě stránky a ukazatel na začátek slotu, a každá
  inicializovaná stránka se vynuluje (poučení: `Vec<Option<Value>>`
  in-place collect přesouvá i neinicializované bajty, takže stará značka z
  předchozího života stránky vyplavala v živém bloku jako falešný double
  free).
- **`RPHP_HEAP=system`** přepne vše na systémový alokátor (A/B bez rebuildu).
- **`--features php-heap-asm`**: ruční x86-64 sekvence rychlé cesty alloc
  (offsety hlavičky jsou přišpendlené `const` asserty).

Invarianty: blok patří přesně jedné stránce; stránka má nejvýš jednoho
vlastníka (`owner_state & !0x3F`); `used == 0` znamená, že nikde neexistuje
vydaný blok stránky (remote bloky se odečítají až při absorpci); stránka je
ve frontě remote vlastníka jen s nastaveným QUEUED a jen dokud je jeho;
osiřelá/poolovaná stránka má stav 0 a její remote list přebírá adoptující;
alokátor nikdy neunwinduje a nealokuje přes sebe.

## Měření (stejný stroj, střídavé běhy, všechny varianty ze stejného commitu)

Zátěž ostatních agentů na stroji kolísá, proto se porovnává jen uvnitř
jednoho běhu skriptu (`measure.sh`, 3–5 kol, každé kolo všechny varianty za
sebou). PHPStan 2.2.14 phar, `analyse` nad `scratchpad/proj`, výstup
byte-identický s PHP ve všech bězích. Wall = `/usr/bin/time %e`, RSS =
`%M`, faulty = `%R` (minor).

**Běh A (commit 7ea29d83, v7, bez THP a velkých bloků), medián z 5 kol:**

| | glibc (`RPHP_HEAP=system`) | mimalloc | heap v7 (Rust) | heap v7 (asm) |
|---|---|---|---|---|
| boot `--version` | 1,20 s / 252 MB | 0,97 s / 286 MB | 1,03 s / 245 MB | 1,03 s / 245 MB |
| cold analyse | 4,25 s / 685 MB | 3,49 s / 735 MB | 3,59 s / 654 MB | 3,60 s / 654 MB |
| warm analyse | 1,51 s / 284 MB | 1,25 s / 312 MB | 1,30 s / 276 MB | 1,30 s / 276 MB |
| allocbench.php | 1,29 s | 1,02 s | 0,98 s | 0,99 s |

**Běh B (stroj méně zatížený; commit s THP + regionem velkých bloků), 3 kola:**

| | mimalloc | heap v7 | heap THP (16 MiB pool) | heap THP + velké bloky | heap, THP vypnuto |
|---|---|---|---|---|---|
| boot | 0,91–1,01 s / 286 MB | 0,95–0,96 s / 246 MB | 0,90–0,92 s / 274 MB | 0,90–0,93 s / 308 MB | 0,95–1,02 s / 267 MB |
| cold | 3,11–3,43 s / 709–737 MB | 3,21–3,22 s / 655 MB | 3,04–3,13 s / 680 MB | **3,01–3,03 s** / 733 MB | 3,21–3,23 s / 690 MB |
| warm | 1,13–1,15 s / 309–314 MB | 1,19–1,22 s / 276 MB | 1,13–1,16 s / 303 MB | **1,11–1,13 s** / 347 MB | 1,20–1,24 s / 304 MB |
| minor faulty cold | 29–39 k | 221 k | 122 k | 110 k | 227 k |
| allocbench | 0,95–1,00 s | 0,94–0,97 s | 0,97–0,99 s | 0,97–1,01 s | 0,96–1,01 s |

**Callgrind (deterministické, instrukce celkem):**

| | glibc | mimalloc | heap (Rust) | heap (asm) |
|---|---|---|---|---|
| allocbench.php | 21,57 G | 18,08 G | **17,81 G** | 18,16 G |
| usestmt.php | 5,517 G | 5,203 G | **5,085 G** | 5,094 G |

Závěry:

- Vlastní heap vykonává méně instrukcí než mimalloc i glibc a na PHPStanu
  je s THP a vlastním regionem velkých bloků nejrychlejší ve všech třech
  metrikách (cold ≈ −4 % až −12 % proti mimallocu podle zátěže, warm a boot
  o 1–3 %). Cena: RSS srovnatelné s mimallocem (733 vs 709–737 MB), o 80 MB
  více než v7 bez THP (2 MiB granularita + držení volných velkých bloků).
- **ASM varianta rychlé cesty není rychlejší**: stejný nebo horší počet
  instrukcí (LLVM z Rust kódu vygeneruje identickou sekvenci; ruční blok
  navíc načítá velikost třídy i na pop cestě a přidává jeden test/jmp) a
  wall time v šumu. Ověřeno disassemblerem `__rust_alloc` obou binárek a
  střídavým měřením. Zůstává za feature `php-heap-asm` jako referenční
  experiment.
- Zbývající rozdíl počtu faultů proti mimallocu (110 k vs 30–39 k) není
  v heapu: ~7 000 `mmap`/`munmap` na cold běh u obou variant jsou zásobníky
  parser vláken (jedno vlákno na soubor), tj. téma pro runtime, ne pro
  alokátor.
- Prolog `__rust_alloc` má 3 push/pop kvůli rozhodnutí LLVM register
  alokátoru při tail-callech do studených cest; přeuspořádání argumentů ani
  skalární argumenty to nezměnily. Release profil není LTO, statiky z lib
  crate jdou přes GOT (sloučeno do jedné statiky `RANGE`); `max-perf`
  profil (fat LTO) tuto nepřímost odstraní.

## Co je ještě otevřené

1. **Typované pooly a inline rychlá cesta na konkrétních místech**
   (`Rc<String>`, `Rc<PhpArray>`, objekty): obchází i `Layout` výpočet.
2. **Jednoalokační řetězce** (hlavička + bajty): největší snížení počtu
   alokací (2,55 M/běh); vyžaduje vlastní owner místo `Rc<String>` a
   společný kontrakt s klíči polí, cache a účtováním (`as_string_mut`
   vrací `&mut String` na stovkách míst).
3. **Bloky > 4 MiB a align > 16**: systémový alokátor (vzácné).
4. **Remote free na stránce, jejíž vlastník už nerefilluje**: bloky čekají
   v `remote_free`, dokud vlastník nepotřebuje paměť (nebo nezanikne).
5. **Prolog `__rust_alloc`**: LLVM ukládá argumenty do callee-saved
   registrů (3 push/pop) kvůli tail-callům do studených cest; přeuspořádání
   argumentů nepomohlo. Fat LTO (`max-perf`) navíc odstraní GOT nepřímost.
6. **Účtování `request_memory`**: nezávislé na alokátoru; vlastní string
   owner by mohl účtovat přímo místo weak hash mapy.

## Testy

`src/heap/tests.rs`: tabulka tříd (malé i střední), LIFO reuse, zeroed/align
kontrakty, realloc, střední třídy na středních stránkách, velké bloky, stress 200 k operací s náhodnými velikostmi a kontrolou
obsahu. `tests/heap_adoption.rs` (vlastní proces): adopce stránek zaniklého
vlákna. `tests/heap_remote_free.rs` (vlastní proces): ping-pong producent /
4 uvolňující vlákna, návrat remote bloků vlastníkovi.
`tests/heap_recycling.rs` (vlastní proces): prázdné stránky jdou do poolu
a obslouží další refill. Velké bloky (region, zaokrouhlená kapacita,
recyklace, hranice k systému) v `src/heap/tests.rs`. Vše běží i s
`php-heap-asm` a `php-heap-debug`.
Celá default e2e sada běží pod heapem (globální alokátor je v lib).
