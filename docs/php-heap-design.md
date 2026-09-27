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

## Architektura v1

- **Rezervace adresního prostoru**: jeden `mmap(MAP_NORESERVE)` 64 GiB
  (fallback poloviční až 256 MiB). Stránky 64 KiB se odřezávají sekvenčně
  (`PAGE_CURSOR`). Vlastnictví bloku = jedno porovnání rozsahu
  (`in_pool`), bez hlaviček bloků.
- **Velikostní třídy** (20): 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112,
  128, 152, 168, 192, 256, 384, 512, 768, 1024. Dvě konstantní tabulky
  `(size+7)/8 → třída` (pro align ≤ 8 a pro align 16, kde třída musí být
  násobek 16). Nad 1 KiB nebo align > 16 → systémový alokátor (bloky leží
  mimo rezervaci, `dealloc` je rozliší rozsahem).
- **Stránka** = jedna třída; hlavička 64 B na začátku: třída, vlastník (tid,
  0 = osiřelá), atomický remote free list, bump kurzor, konec, link do
  seznamu stránek vlastníka, ukazatel na heap vlastníka, link do fronty
  „stránky s remote free“, příznak zařazení.
- **Thread heap** (`thread_local!` bez destruktoru → přímý `fs:` přístup):
  per třída LIFO free list (intrusive, první slovo bloku), aktuální stránka
  pro bump, seznam stránek; `mode` (pool/system) zkopírovaný do TLS bloku;
  atomická fronta stránek s remote free.
- **Rychlá cesta alloc**: TLS → tabulka tříd → pop free listu; když prázdný,
  bump z aktuální stránky; jinak tail-call do studeného `refill_or_system`
  (drain fronty remote stránek → adopce osiřelé stránky → nová stránka →
  systémový fallback).
- **Rychlá cesta free**: rozsah → hlavička stránky → `owner == tid` → push
  na lokální free list; jinak CAS push na remote list stránky a při prvním
  bloku zařazení stránky do fronty vlastníka (`owner_heap`).
- **Zánik vlákna**: `EXIT_GUARD` (TLS s destruktorem, registrovaný při
  prvním použití) přesune všechny stránky do globálního orphan poolu
  (mutex; přeskočen, když `ORPHAN_COUNT == 0`), free listy vrátí do remote
  listů stránek. Adoptující vlákno přebírá vlastnictví a remote bloky.
- **realloc**: ve stejné třídě vrací tentýž blok; jinak alloc+copy+free.
  Systémový blok zůstává u systému.
- **`RPHP_HEAP=system`** přepne vše na systémový alokátor (A/B bez rebuildu).

Invarianty: blok patří přesně jedné stránce; stránka má nejvýš jednoho
vlastníka; osiřelá stránka má `owner == 0` a `owner_heap == 0`, její remote
list vlastní adoptující; free listy vlákna obsahují jen bloky stránek, které
vlastní; alokátor nikdy neunwinduje a nealokuje přes sebe.

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

## Co je ještě otevřené (podle review)

1. **Vracení paměti OS / plně volné stránky**: v1 stránky neuvolňuje. Nutné
   pro dlouhé běhy: per-page počet použitých bloků (owner only) + `madvise`
   nebo návrat stránky do globálního volného seznamu stránek.
2. **Zánik vlákna po `EXIT_GUARD`**: alokace v pozdějších TLS destruktorech
   téhož vlákna vytvoří stránky, které už nikdo neosiří (malý, ohraničený
   únik). Řešení: druhý průchod nebo lazy orphaning při dalším refillu.
3. **Velké bloky**: systémový alokátor; pro FPM-styl a velké pole zvážit
   vlastní správu > 1 KiB (segmenty, bitmapy).
4. **Typované pooly a inline rychlá cesta na konkrétních místech**
   (`Rc<String>`, `Rc<PhpArray>`, objekty): obchází i `Layout` výpočet.
5. **Jednoalokační řetězce** (hlavička + bajty): největší snížení počtu
   alokací (2,55 M/běh); vyžaduje vlastní owner místo `Rc<String>` a
   společný kontrakt s klíči polí, cache a účtováním.
6. **ASM varianta rychlé cesty**: porovnat disassembler Rust verze
   s ruční sekvencí na stejné struktuře; přijmout jen s měřitelným zlepšením.
7. **Debug režim**: kanárky, otrávení, detekce double-free; sanitizer nezná
   hranice slotů uvnitř stránky.
8. **Účtování `request_memory`**: nezávislé na alokátoru; vlastní string
   owner by mohl účtovat přímo místo weak hash mapy.

## Testy

`src/heap/tests.rs`: tabulka tříd, LIFO reuse, zeroed/align kontrakty,
realloc, stress 200 k operací s náhodnými velikostmi a kontrolou obsahu.
`tests/heap_adoption.rs` (vlastní proces): adopce stránek zaniklého vlákna.
Celá default e2e sada běží pod heapem (globální alokátor je v lib).
