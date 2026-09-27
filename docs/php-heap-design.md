# PHP heap (`rphp::heap`): návrh a stav

Větev `codex/php-heap-v1` (základ main `9439a84a`). Cíl zadaný uživatelem:
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

## Architektura (stav po review, commit `30ab4986`)

- **Rezervace adresního prostoru**: jeden `mmap(MAP_NORESERVE)` 64 GiB
  (fallback poloviční až 256 MiB), báze zarovnaná na 64 KiB,
  `madvise(MADV_HUGEPAGE)` na celý rozsah (`RPHP_HEAP_THP=0` vypne).
  Rozložení od báze: pole hlaviček stránek, malé stránky (64 KiB) do
  poloviny, střední stránky (1 MiB) do tří čtvrtin, velké bloky (násobky
  64 KiB do 4 MiB) v poslední čtvrtině. Vlastnictví bloku = jedno odečtení
  a porovnání s rozsahem, žádné hlavičky bloků u malých a středních tříd.
- **Pole hlaviček stránek**: 128 B na každou 64KiB jednotku (střední
  stránka používá záznam své první jednotky), hlavička se najde aritmetikou
  z offsetu bloku. Dřív ležela hlavička na začátku každé stránky, tedy na
  adrese zarovnané na 64 KiB: všechny hlavičky padaly do jedné množiny L1
  (8 cest) a do jedné či dvou množin L2. Smyčka střídající deset velikostí
  měla podle cachegrindu 3,8 M minutí L1 na 2 M iterací (mimalloc 16 k);
  s polem hlaviček 3,2 k. První blok stránky navíc začíná na jednom z 16
  posunů po 64 B podle adresy stránky, aby se nekryly ani nejteplejší bloky.
- **Velikostní třídy** (40): malé 8 … 1024 podle hodnot RPHP (40 B
  `RcBox<String>`, 48 B tři `Value`, 152 B `RcBox<PhpArray>` …), tabulka
  indexovaná přímo velikostí (zvlášť pro align 16); střední 1280 … 32768 ve
  čtyřech krocích na oktávu. Velké bloky 32 KiB–4 MiB: 128B hlavička
  s počtem jednotek, zásobník volných bloků na každý počet jednotek. Nad
  4 MiB nebo align > 16 systémový alokátor.
- **Stránka** = jedna třída. Horký řádek hlavičky: `local_free` (LIFO bloků
  uvolněných vlastníkem), `bump`, `end`, `used`, `block_size`, `owner`
  (adresa heapu vlastníka), `class`, `listing`, odkazy seznamu dostupných
  stránek. Druhý řádek (zapisují ho jiná vlákna): `owner_state` (heap |
  QUEUED | počet pusherů), `remote_free`, `next_queued`, odkazy seznamu
  všech stránek vlastníka, `start`. `used` nese navíc `CURRENT_BIAS` (+1),
  dokud je stránka aktuální, a příznak `RETIRED`, dokud není nikde zařazená.
- **Thread heap**: na x86-64 Linuxu vlastní TLS blok v `global_asm!`
  s přístupem local-exec (`mov fs:0` + `lea sym@tpoff`, bez clobberů).
  `thread_local!` v této knihovně se překládá jako general-dynamic TLS:
  volání `__tls_get_addr`, které linker až dodatečně přepíše na dvě
  instrukce, ale překladač už předtím uložil a přeházel registry argumentů.
  To byly tři push/pop v prologu `__rust_alloc`, které dřívější verze
  dokumentu mylně připisovala alokaci registrů (týká se všech
  `thread_local!` v runtime, 15 z 18 objektových souborů knihovny). Jiné
  platformy používají `thread_local!`. Pro třídu bez stránky ukazuje
  `current[class]` na read-only sentinel stránku, ze které pop ani bump
  nikdy neuspěje (žádný test na null). Heap si kešuje bázi a velikost
  malého regionu (žádný globální load při free).
- **Rychlá cesta alloc** (Rust, inlinovaná do míst volání): align ≤ 8
  a velikost ≤ 1024 → třída z tabulky → aktuální stránka → pop
  z `local_free`, jinak bump; `used += 1`. Při známé velikosti překladač
  výběr třídy úplně vypustí. Jinak tail-call do studených cest
  (`alloc_other` pro align 16, střední a velké bloky; `alloc_slow` pro
  refill: drain vzdálených free → aktuální stránka → další dostupná →
  adopce osiřelé → stránka z poolu (rezidentní, pak vrácená OS) → nová).
- **Rychlá cesta free**: offset od kešované báze < velikost malého regionu
  → hlavička z pole → `owner == heap` → push na `local_free`, `used -= 1`;
  jediný znaménkový test `used <= 0` pokryje „dostupná stránka se
  vyprázdnila“ i „stránka byla plná a nezařazená“ (aktuální stránka díky
  biasu nikdy nedojde k nule). Cizí blok → CAS na `remote_free` stránky
  a zařazení stránky do fronty vlastníka pod ochranou počtu pusherů.
- **Protokol vlastnictví**: remote uvolňovatel CAS-em nastaví QUEUED
  a zvýší počet pusherů, teprve pak sáhne na heap vlastníka. Vlastník před
  vzdáním se stránky (`detach`/`try_detach`) čeká na nulový počet pusherů;
  `try_detach` odmítne, dokud je stránka QUEUED.
- **Pooly volných stránek a velkých bloků**: lock-free zásobníky, jejichž
  hlava nese 24bitový index jednotky a 40bitovou verzi zvyšovanou každou
  změnou (odolné vůči ABA). Odkaz leží v záznamu pole hlaviček (u velkého
  bloku v jeho hlavičce), nikdy v paměti bloku.
- **Vracení paměti OS**: nad rozpočet (64 MiB stránek na region, 256 MiB
  velkých bloků) se paměť vrací přes `madvise(MADV_DONTNEED)` se začátkem
  zarovnaným na stránku OS (stránka celá, velký blok bez první stránky OS
  s hlavičkou) a položka jde do „studeného“ zásobníku, ze kterého se bere až
  po rezidentních.
- **Zánik vlákna**: `EXIT_GUARD` (TLS s destruktorem, registrovaný před
  první stránkou; když registrace selže, vlákno stránky nebere) přesune
  prázdné stránky do poolu a ostatní do orphan seznamu, vlákno pak alokuje
  systémově.
- **realloc**: ve stejné třídě vrací tentýž blok; velký blok zůstává, dokud
  nová velikost potřebuje velký region a využije víc než polovinu
  kapacity; jinak alloc + copy + free.
- **`php-heap-asm`** (volitelné, x86-64 Linux): rychlé cesty jako naked
  funkce; `__rust_alloc`/`__rust_dealloc` na ně skočí, listy čtou heap přes
  `%fs` a při neúspěchu skočí s nedotčenými registry do stejných Rust
  studených cest. Viz závěr k ASM níže.
- **Debug režim** (`php-heap-debug`): bitmapa volných slotů (1 bit na 8 B
  rezervace) místo značky v bloku; double free → abort s popisem bloku,
  jed za odkazovým slovem odhalí zápis po uvolnění, kontrola layoutu
  a začátku slotu. Značka uložená v bloku dávala falešné poplachy:
  `Vec<Option<T>>::clone` kopíruje neinicializované bajty payloadu `None`,
  které mohly nést hodnotu značky ponechanou alokátorem v registru.
- **`RPHP_HEAP=system`** přepne na systémový alokátor, ale není to nulová
  režie (rychlá cesta proběhne a skončí ve studené cestě). Pro srovnání
  s glibc se měří build bez feature `php-heap`.

Invarianty: blok patří přesně jedné stránce; stránka má nejvýš jednoho
vlastníka; `used` (bez příznaků a biasu) = vydané bloky včetně čekajících
v `remote_free`; stránka je ve frontě vlastníka jen s nastaveným QUEUED
a jen dokud je jeho; osiřelá či poolovaná stránka má stav 0; odkazová slova
zásobníků jsou metadata dostupná jen atomicky; alokátor nikdy neunwinduje
a nealokuje přes sebe.

## Review z 27. 9. 2026 a opravy

| Nález | Oprava | Test |
|---|---|---|
| ABA v zásobnících volných stránek a velkých bloků (review reprodukovalo dvojí vydání živého bloku) | hlava = index + verze, odkaz mimo paměť bloku | deterministické proložení z review, souběžné testy s kontrolou obsahu |
| `madvise` na hlavička + 128 B vracel `EINVAL` a výsledek se ignoroval | začátek zarovnaný na stránku OS, počítadlo vrácených bajtů | `mincore` na vrácené oblasti, rozpočtový test s poklesem RSS |
| ASM načítal velikost třídy i při popu a překladač znovu testoval výsledek | ASM přestavěn na naked listy, velikost jen v bump větvi | disassembler, instrukce na iteraci |
| ASM + debug nekontroloval stejné věci | debug build vždy používá Rust cesty | `--all-features` sada |
| Srovnání s glibc přes `RPHP_HEAP=system` a tabulka ze dvou různých běhů | build bez `php-heap`, jeden střídavý běh všech variant | viz měření |
| Vlastní: přetečení kurzoru regionu (`fetch_add`/`fetch_sub`) mohlo dát dvěma vláknům překrývající se rozsah | CAS smyčka | souběžný test na hranici |
| Vlastní: general-dynamic TLS (prolog s push/pop) | vlastní TLS blok local-exec | disassembler |
| Vlastní: aliasing hlaviček v L1/L2 | pole hlaviček, posun prvního bloku | test rozptylu, cachegrind |
| Vlastní: falešné double free v debug režimu | bitmapa volných slotů | testy detektorů v podprocesu |

Moje chyba během měření: první mikrobenchmarky jsem sestavil výchozím
Rustem 1.93.1 (sonda mimo repozitář neměla `rust-toolchain.toml`), zatímco
RPHP používá 1.98.1. Všechna čísla níže jsou z 1.98.1, ověřeno
`readelf -p .comment` u každé binárky.

## Měření (27. 9. 2026, rustc 1.98.1, release)

Stroj: Ryzen 9 7950X, THP v režimu `madvise`. Všechny varianty RPHP ze
stejného zdroje (`30ab4986`, glibc = build bez `php-heap`, mimalloc =
`#[global_allocator]` v `main.rs`), jen „heap před review“ je `f20fa66c`.
Varianty se střídají v každém kole (rotace pořadí), měření pod sdíleným
zámkem `/tmp/rphp-benchmark.lock`. PHPStan 2.2.14 phar nad
`scratchpad/proj`; výstup byte-identický s PHP ve všech 150 bězích.

**PHPStan a PHP skripty, medián z 5 kol (čas / maximální RSS):**

| | glibc | mimalloc | heap před review | heap Rust | heap ASM |
|---|---:|---:|---:|---:|---:|
| boot `--version` | 1,17 s / 246 MB | 0,95 s / 280 MB | 0,96 s / 300 MB | 0,95 s / 306 MB | 0,94 s / 304 MB |
| cold `analyse` | 4,06 s / 668 MB | 3,39 s / 709 MB | 3,27 s / 714 MB | 3,23 s / 722 MB | 3,21 s / 720 MB |
| warm `analyse` | 1,46 s / 277 MB | 1,21 s / 306 MB | 1,19 s / 339 MB | 1,17 s / 344 MB | 1,18 s / 342 MB |
| cold s workery (bez `-d disable_functions`) | 7,49 s | 6,22 s | 5,92 s | 5,87 s | 5,75 s |
| allocbench.php | 1,20 s | 1,03 s | 1,00 s | 0,97 s | 0,96 s |
| usestmt.php | 0,41 s | 0,34 s | 0,35 s | 0,33 s | 0,33 s |
| minor faulty cold | 224 k | 36 k | 111 k | 111 k | 110 k |

Poměr ASM/Rust po kolech: cold 0,95–1,01 (medián 0,99), warm 0,93–1,03
(medián 1,00), s workery 0,90–1,00 (ASM rychlejší ve všech pěti kolech,
medián 0,97). Cold a warm jsou nerozhodné, s workery má ASM malý náskok.

**Sada `benches/bench_*.php`** (96 skriptů, které doběhnou za 0,25–4 s,
3 kola, všechny výstupy v pořádku): celkem glibc 51,4 s, mimalloc 49,4 s,
heap před review 50,2 s, heap Rust 49,0 s, heap ASM 49,2 s; geometrický
průměr proti glibc 0,971 / 0,991 / 0,970 / 0,970. Většina skriptů jsou
skalární smyčky bez alokací.

**Mikrobenchmark z review** (stejný driver, 9 kol, pevné CPU, 50 M párů):

| | glibc | mimalloc | heap před review | heap Rust | heap ASM |
|---|---:|---:|---:|---:|---:|
| lifo | 367 ms | 210 ms | 198 ms | **157 ms** | 178 ms |
| mixed | 398 ms | 277 ms | 381 ms | 202 ms | **200 ms** |
| burst | 431 ms | 215 ms | 232 ms | **173 ms** | 179 ms |
| instrukce/iteraci lifo | 172 | 98 | 102 | 67 | 63 |

Instrukce na nejčastější cestě (finální binárky, včetně vstupního skoku
u ASM): alloc Rust 16 / ASM 15 (review: 34 / 38), free Rust 22 / ASM 18
(review: 38 / 38; pole hlaviček přidalo dvě instrukce za dohledání
hlavičky).

**Ověření dříve tvrzeného:**

- Huge pages (špička `AnonHugePages` během cold běhu): glibc 0 MB,
  mimalloc 672 MB, heap 504 MB. Dřívější tvrzení, že mimalloc huge pages
  nedostane, bylo chybné.
- Cold běh udělá 868 vláken parseru (64MiB zásobníky, 868 × mmap/munmap),
  869 alternativních zásobníků pro signály a 5 228 × mmap/munmap 8MiB
  segmentů zásobníku (`stacker`). Dřívější připsání všech ~7 000 volání
  vláknům bylo nepřesné. Týká se runtime, ne alokátoru.
- Zbytek faultů proti mimallocu tvoří hlavně systémové alokace nad 4 MiB
  (2 × 29 MB, 2 × 32 MB), které heap posílá do glibc bez huge pages.

## ASM: závěr

Naked funkce nejde inlinovat (vlastní `ret`, skoky do jiných symbolů),
takže každá alokace zaplatí `call`, nepřímý skok přes GOT a `ret`. Inline
`asm!` inlinovat jde, ale je pro optimalizátor neprůhledný (nevidí dovnitř,
nemůže předpočítat třídu, výsledek testuje znovu). Rust cesta se inlinuje
do ~22 000 míst a u známé velikosti vypustí výběr třídy; stojí to 1,7 MB
kódu navíc. Výsledek: ASM vykoná méně instrukcí; v čase vyhrává Rust
v mikrobenchmarku lifo (−12 %) a burst (−3 %), mixed je nerozhodný. Na
PHPStanu jsou cold a warm nerozhodné a v běhu s workery je ASM asi o 3 %
rychlejší; sada benchmarků vychází stejně. Jednoznačný vítěz tedy není.
Výchozí zůstává Rust (přenositelný, s debug kontrolami, bez ručně
udržovaných offsetů); `php-heap-asm` zůstává volitelný.

## Co je ještě otevřené

1. **Typované pooly a jednoalokační řetězce** (`Rc<String>` = dvě alokace,
   účtování přes weak hash mapu): největší další snížení počtu alokací;
   vyžaduje změnu vlastnictví řetězců ve `Value`.
2. **Bloky nad 4 MiB** přes vlastní region s 2MiB granularitou (THP,
   opakované použití místo mmap/munmap); pozor na `mremap` u glibc realloc.
3. **TLS model v celém runtime** (samostatný úkol): stejný prolog platí
   u každého `thread_local!` v knihovně.
4. **Vlákna a zásobníky parseru** (samostatný úkol).
5. **Politika inlinování**: inline alloc (skládání konstant) a mimo řádek
   free (méně kódu) by mohla spojit výhody obou variant; neměřeno.
6. **`max-perf`** (fat LTO) zatím neměřeno.
7. **Remote free** čeká v `remote_free`, dokud vlastník nerefilluje.

## Testy

`src/heap/tests.rs`: tabulky tříd, LIFO reuse, zeroed/align kontrakty,
realloc, střední a velké bloky, sentinel, ABA proložení, přetečení verze,
`mincore` po vrácení paměti, souběžný carve, retire/relist/recycle, rozptyl
hlaviček, stress 200 k operací. Integrační testy ve vlastních procesech:
`heap_adoption` (osiřelé stránky), `heap_remote_free`, `heap_recycling`,
`heap_concurrency` (8 vláken churn, producenti/konzumenti, generace vláken,
kontrola obsahu každého bloku), `heap_reclaim` (rozpočet, pokles RSS),
`heap_debug` (double free a zápis po uvolnění v podprocesu, kopie
neinicializovaných bajtů bez falešného poplachu). Vše běží pro Rust, ASM
i debug variantu; celá e2e sada běží pod heapem.
