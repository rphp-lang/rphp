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
  (fallback poloviční až 256 MiB). Dolní polovina = region malých stránek
  (64 KiB), horní polovina = region středních stránek (1 MiB). Vlastnictví
  bloku = jedno odečtení a porovnání s rozsahem (`pool_offset`), hlavička
  stránky = maska podle regionu; žádné hlavičky bloků. Tři rozsahové
  hodnoty leží v jedné 64 B statice (`RANGE`), takže rychlá cesta bez LTO
  dělá jediný GOT load.
- **Velikostní třídy** (40): malé 8, 16, 24, 32, 40, 48, 56, 64, 80, 96,
  112, 128, 152, 168, 192, 256, 384, 512, 768, 1024 (dvě konstantní tabulky
  `(size+7)/8 → třída`, pro align ≤ 8 a pro align 16); střední 1280 … 32768
  ve čtyřech krocích na oktávu (1,25×, 1,5×, 1,75×, 2×), třída spočtená
  z `bsr`. Nad 32 KiB nebo align > 16 → systémový alokátor (bloky leží mimo
  rezervaci).
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
  hlavička se přepíše). Nad rozpočet 16 MiB rezidentních stránek na region
  se blokový prostor stránky vrací OS přes `madvise(MADV_DONTNEED)`.
- **Zánik vlákna**: `EXIT_GUARD` (TLS s destruktorem, registrovaný při
  první stránce) přesune plně volné stránky do poolu a ostatní do orphan
  seznamu (mutex, přeskočen při `ORPHAN_COUNT == 0`), přepne vlákno do
  režimu system (pozdější TLS destruktory alokují u systému, nic neuniká).
- **realloc**: ve stejné třídě vrací tentýž blok; jinak alloc+copy+free.
  Systémový blok zůstává u systému.
- **Debug režim** (`--features php-heap-debug`): uvolněný blok otráven
  0xDE, druhé slovo nese značku; opakované uvolnění téhož bloku → abort
  s hlášením.
- **`RPHP_HEAP=system`** přepne vše na systémový alokátor (A/B bez rebuildu).
- **`--features php-heap-asm`**: ruční x86-64 sekvence rychlé cesty alloc
  (offsety hlavičky jsou přišpendlené `const` asserty).

Invarianty: blok patří přesně jedné stránce; stránka má nejvýš jednoho
vlastníka (`owner_state & !0x3F`); `used == 0` znamená, že nikde neexistuje
vydaný blok stránky (remote bloky se odečítají až při absorpci); stránka je
ve frontě remote vlastníka jen s nastaveným QUEUED a jen dokud je jeho;
osiřelá/poolovaná stránka má stav 0 a její remote list přebírá adoptující;
alokátor nikdy neunwinduje a nealokuje přes sebe.

## Měření (stejný stroj, sekvenčně; zátěž ostatních agentů kolísá)

| | glibc | mimalloc | heap v2 | heap v3 | heap v5 (Rust) | heap v5 (asm) |
|---|---|---|---|---|---|---|
| boot | 1,29 s | 1,10 / 0,91 s | 1,13 s | 1,16 s | 0,94 s | 0,94 s |
| cold | 5,14 s | 4,41 / 3,21 s | 4,31 s | 3,69 s | 3,28–3,32 s | 3,25–3,27 s |
| warm | 1,70 s | 1,38 / 1,14 s | 1,49 s | 1,26–1,36 s | 1,21–1,23 s | 1,22 s |
| RSS cold | 684 MB | 723–734 MB | 649 MB | 650 MB | 649 MB | 649 MB |

(dvě hodnoty mimalloc = dvě různé zátěže stroje; porovnávat vždy jen
sousední sloupce téhož běhu; v5 a asm byly měřeny prokládaně se stejným
mimalloc během: 3,21 / 1,14 s). Instrukce alokátoru v benchmarku smyčky
`usestmt.php`: glibc 10 % → heap v3+ < 0,3 % (rychlá cesta inlinovaná do
volajících; callgrind celkem 5,49 G glibc → 5,11 G heap → 5,12 G heap-asm).

Rychlá cesta v5 (`__rust_alloc`, hit ve free listu): 22 instrukcí včetně
3 push/pop kvůli tail-callům do studených cest; mimalloc srovnatelně.
**ASM varianta (`php-heap-asm`, x86-64 inline asm pro pop/bump nad stejnou
strukturou) dává shodný čas i počet instrukcí** — kompilátor generuje tutéž
sekvenci; jediné, co asm změnilo, je pořadí načtení vstupů. Závěr měření:
další zisk je v politice (per-page free listy pro lokalitu, typované pooly
s třídou známou při překladu, méně alokací), ne v instrukčním výběru.

Zbývající rozdíl proti mimalloc (~2–6 % času při −12 % RSS) je
pravděpodobně lokalita: náš free list třídy míchá bloky ze všech stránek
vlákna, mimalloc alokuje ze seznamu jedné stránky, dokud ji nevyčerpá
(lepší TLB/cache pro po sobě jdoucí alokace). To je další měřený krok.

v1 (refill při každém bumpu, průchod všech stránek) byl 8× pomalejší než
glibc — připomínka, že politika, ne instrukce, rozhoduje.

## Co je ještě otevřené

1. **Typované pooly a inline rychlá cesta na konkrétních místech**
   (`Rc<String>`, `Rc<PhpArray>`, objekty): obchází i `Layout` výpočet.
2. **Jednoalokační řetězce** (hlavička + bajty): největší snížení počtu
   alokací (2,55 M/běh); vyžaduje vlastní owner místo `Rc<String>` a
   společný kontrakt s klíči polí, cache a účtováním (`as_string_mut`
   vrací `&mut String` na stovkách míst).
3. **Bloky > 32 KiB**: systémový alokátor (0,1 % alokací); vlastní správa
   by dávala smysl až pro FPM-styl workloady.
4. **Remote free na stránce, jejíž vlastník už nerefilluje**: bloky čekají
   v `remote_free`, dokud vlastník nepotřebuje paměť (nebo nezanikne).
5. **Prolog `__rust_alloc`**: LLVM ukládá argumenty do callee-saved
   registrů (3 push/pop) kvůli tail-callům do studených cest; přeuspořádání
   argumentů nepomohlo. Fat LTO (`max-perf`) navíc odstraní GOT nepřímost.
6. **Účtování `request_memory`**: nezávislé na alokátoru; vlastní string
   owner by mohl účtovat přímo místo weak hash mapy.

## Testy

`src/heap/tests.rs`: tabulka tříd (malé i střední), LIFO reuse, zeroed/align
kontrakty, realloc, recyklace prázdných stránek přes pool, střední třídy na
středních stránkách, stress 200 k operací s náhodnými velikostmi a kontrolou
obsahu. `tests/heap_adoption.rs` (vlastní proces): adopce stránek zaniklého
vlákna. `tests/heap_remote_free.rs` (vlastní proces): ping-pong producent /
4 uvolňující vlákna, návrat remote bloků vlastníkovi. Vše běží i s
`php-heap-asm` a `php-heap-debug`.
Celá default e2e sada běží pod heapem (globální alokátor je v lib).
