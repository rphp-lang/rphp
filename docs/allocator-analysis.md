# Vlastní alokátor pro RPHP: analýza (2026-09-27)

> Historical hypothesis, superseded by the implemented heap and its measurements.
> In particular, the blanket claim that Rust global allocation cannot inline
> was disproved by inspection of the generated runtime. Instruction ceilings
> and time estimates below are historical estimates, not current guarantees.
> See [the Rust heap checkpoint](performance-php-heap-finish.md) and
> [the integration/PHPStan comparison](performance-php-heap-integration.md).

Otázka: dá se napsat vlastní alokátor (ideálně s částmi v assembleru), který
výkonově překoná stávající alokátory (glibc malloc, mimalloc) na zátěži
RPHP, konkrétně na PHPStanu?

Zadání pro tuto verzi: **unsafe policy repa ignorovat; cílem je absolutně
překonat výkon všech existujících alokátorů na zátěži RPHP.** Analýza proto
nejdřív spočítá, kolik je vůbec k dispozici (strop), a pak popíše design,
který se ke stropu přiblíží, včetně toho, co může a nemůže dát assembler.

Krátká odpověď: glibc lze překonat výrazně (mimalloc to už dělá: −20 % času);
strop nad mimalloc je na této zátěži **~1,0 G instrukcí (3,4 %) a odhadem
3–6 % času**, protože alokátor už je jen 5,6 % běhu. Ten strop se dobývá
politikou (žádný cross-thread free, žádné hlavičky, velikostní třídy na míru,
inline rychlá cesta u pěti nejhorčejších míst), ne instrukčním výběrem;
assembler nemá co zkrátit. Násobně větší výnos je ve snížení počtu alokací.

## 1. Změřená východiska

Cold běh PHPStanu (`analyse`, 29,8 G instrukcí, callgrind, binárka main 43ede4ab):

| alokátor | instrukce v alokátoru | podíl | wall cold | wall warm | boot | RSS cold |
|---|---|---|---|---|---|---|
| glibc malloc | 4,9 G (`_int_malloc` 1,47, `_int_free` 1,31, `malloc` 1,14, `free` 0,56, `consolidate` 0,26, realloc/calloc 0,17) | 16,5 % | 4,63 s | 1,57 s | 1,23 s | 684 MB |
| mimalloc (větev `codex/perf-allocator-experiment`) | 1,55 G (`mi_theap_malloc_aligned` 0,51, `mi_free` 0,47, `_mi_page_malloc_zero` 0,30, `mi_malloc_aligned` 0,16, generic 0,08) | 5,6 % (z 27,9 G) | 3,68 s | 1,25 s | 0,99 s | 733 MB |

Počet alokací: bootstrap má 8,68 M volání `__rust_alloc` na 8,4 G instrukcí
(1 alokace na ~970 instrukcí); cold běh tedy odhadem 30 M párů alloc/free
(DHAT histogram velikostí a životností běží; doplní se do §7).

Z toho plyne cena jednoho páru alloc+free:

- glibc: ~160 instrukcí,
- mimalloc: ~52 instrukcí (z toho rychlá cesta `malloc` ~17, `free` ~16,
  zbytek stránkové doplňování a zarovnané varianty).

Wall-clock rozdíl (−20 %) je větší než rozdíl instrukcí (−6 %): rozhoduje
lokalita cache (mimalloc vrací nedávno uvolněné bloky stejné velikosti, glibc
míchá velikosti přes tcache/fastbins/unsorted a konsoliduje) a méně špatně
predikovaných větví. **To je klíčové pro celou otázku: čas alokátoru je
paměťový, ne instrukční.**

Co runtime alokuje (velikosti hlaviček bez payloadu):

| objekt | velikost | poznámka |
|---|---|---|
| `Value` | 16 B | prvky `Vec<Value>` (pole, sloty rámců, argumenty) |
| `RcBox<String>` | 40 B + buffer | **každý PHP řetězec = 2 alokace** (hlavička + buffer) |
| `RcBox<PhpArray>` | 16 + 136 B | + `Vec` storage (`ArrayStorage` 104 B inline) |
| `RcBox<RefCell<PhpObject>>` | 16 + 8 + 88 B | + `Vec<Value>` slotů |
| `Token` | 40 B v `Vec<Token>` | + `String` payload identifikátorů |
| AST (`Stmt` 352 B, `Expr` 104 B, `Box<...>`) | | žije jen do konce kompilace souboru |

Životnost: převažují krátce žijící bloky (statement temporaries, argumenty,
tokeny, AST po kompilaci, mezivýsledky řetězcových funkcí). Dlouho žijí
zkompilované op arrays, třídy, kontejner DI.

Vlákna: parser běží u prakticky každého souboru v pomocném vlákně
(`DEDICATED_STACK_NESTING = 5`, `std::thread::scope` v `Parser::parse`) a AST
se uvolňuje v hlavním vlákně. **Cross-thread free je běžný případ**, ne
výjimka; alokátor bez jeho podpory nelze použít.

Report paměti (`memory_get_usage`, limit) jde přes vlastní účtování
(`request_memory`), na alokátoru nezávisí. Výměna alokátoru nemění PHP
sémantiku.

## 2. Co znamená „překonat“ mimalloc

Rychlá cesta mimalloc pro malý blok: TLS heap → index size class (shift +
tabulka) → pop z free listu stránky (2 čtení, 1 zápis) → return. To je ~12
instrukcí; `free` je adresa → hlavička stránky maskováním → push (2 zápisy)
+ test vlastnického vlákna. Teoretické minimum pro obecný alokátor s
oddělenými velikostními třídami je ~6–8 instrukcí na `alloc` a ~5 na `free`.

Horní hranice zisku *v instrukcích* proti mimalloc: 1,55 G → ~0,9 G, tj.
~2 % běhu. Wall-clock zisk z lepší lokality: realisticky do 5 %. Z 3,68 s
tedy nejlépe ~3,5 s. Proti glibc je zisk celý (−20 %), ale ten už mimalloc
dává hotový.

Kde by vlastní alokátor mohl mimalloc skutečně předběhnout (jen politika,
ne kód):

1. **Bez cross-thread mechanismu** — pokud parser přestane běžet v pomocném
   vlákně (přepis rekurzivního parseru na explicitní zásobník nebo větší
   hlavní zásobník), stačí čistě thread-local alokátor bez atomik a bez
   vlastnických testů ve `free`. Přepis parseru se ale vyplatí i sám o sobě.
2. **Bump alokace pro front-end** — tokeny a AST jednoho souboru alokovat z
   arény a po kompilaci zahodit celou (jeden `free` místo ~1,5 M). To ale není
   globální alokátor, to je typovaná aréna (`bumpalo`-styl) v parseru a
   kompilátoru; jde udělat v bezpečném Rustu nad `Vec<u8>`/`typed-arena`.
3. **Žádné hlavičky bloků** — velikost odvozená z adresy stránky (mimalloc
   to dělá, glibc ne). Nutná podmínka, žádný náskok.
4. **Specializované třídy pro naše velikosti** (16, 40, 56, 112, 152 B) místo
   geometrických tříd: ušetří ~10 % paměti a zlepší hustotu v cache. Přínos
   jednotky procent na alokačních cestách.
5. **Vynechání uvolňování při konci procesu** — CLI proces končí, OS paměť
   uklidí; destruktory je ale nutné zavolat kvůli sémantice (ty tvoří
   shutdown 0,4 s), samotné `free` je z toho malý zlomek.

## 2b. Absolutní strop a design, který se k němu přiblíží

Rozpočet: mimalloc utratí 1,55 G instrukcí za ~30 M párů alloc/free (≈52 na
pár). Teoretické minimum obecného alokátoru se segregovanými třídami a bez
hlaviček je ~12 instrukcí na pár (viz sekvence níže), tj. ~0,36 G. Realisticky
dosažitelných je ~0,6 G (doplňování stránek, `realloc`, velké bloky, zeroed
alokace). **Maximální zisk proti mimalloc: ~0,9–1,0 G instrukcí = 3,4 % běhu;
v čase odhadem 3–6 %** (lepší lokalita, žádná atomika, žádné vlastnické
testy). Z 3,68 s cold tedy nejlépe ~3,5 s. Vůči glibc je zisk celých ~1 s, ten
však dává i mimalloc.

Rychlá cesta, ke které se sbíhá každý dobrý alokátor (x86-64, 16 B blok):

```
mov   rax, fs:[HEAP_OFF + CLASS*8]   ; hlavička free listu třídy (TLS, 1 instr.)
test  rax, rax
jz    .refill                        ; prázdný list → bump ze stránky
mov   rcx, [rax]                     ; další volný blok
mov   fs:[HEAP_OFF + CLASS*8], rcx
ret
```

Pět instrukcí, když je size class známá při překladu (konstantní velikost
u `Rc<String>`, `Rc<PhpArray>`, objektů). Pro obecný `alloc(size)` přibude
výpočet třídy (`shr` + tabulka, 3 instrukce). `free` je zrcadlově 3–4
instrukce, pokud nekontroluje vlastnické vlákno. Rust z `unsafe` kódu
generuje přesně tuto sekvenci (ověřitelné `cargo asm`); assembler zde nemá
co ušetřit. Jediné, co kompilátor neumí a asm ano, je vyhradit registr pro
ukazatel na heap místo `fs:`-relativního čtení — to by vyžadovalo, aby celý
interpretr běžel v asm s vlastní konvencí registrů; jedno TLS čtení je
1 instrukce a ~1 cyklus, úspora je nulová.

Design pro dosažení stropu (bez ohledu na unsafe policy):

1. **Thread-local, bez atomik.** Podmínka: parser nesmí běžet v pomocném
   vlákně (přepis rekurze na explicitní zásobník nebo dostatečně velký
   hlavní zásobník), ostatní pomocná vlákna (`crypt`, coroutine resolver)
   dostanou vlastní heap a nikdy neuvolňují cizí bloky. Odpadá test vlastníka
   ve `free` a odložené listy.
2. **Segregované stránky bez hlaviček bloků.** Stránka 64 KB = jedna
   velikostní třída; `free(p)` zjistí třídu z hlavičky stránky (`p &
   !0xFFFF`). Nulová paměťová režie na blok, hustší cache.
3. **Velikostní třídy na míru RPHP**: 16, 32, 40 (`RcBox<String>`), 48, 56,
   64, 80, 96, 112 (`RcBox<RefCell<PhpObject>>`), 128, 152 (`RcBox<PhpArray>`),
   192, 256, 384, 512, 768, 1024; nad 1 KB `mmap`/systém. Interní
   fragmentace < 10 % místo ~25 % u geometrických tříd.
4. **Inline rychlá cesta na horkých místech.** `__rust_alloc` brání inliningu
   globálního alokátoru; horká místa (tvorba `Rc<String>`, `Rc<PhpArray>`,
   objektů, `Vec<Value>` do 8 prvků) proto volají přímo typované pooly
   (`#[inline(always)]`), globální `GlobalAlloc` zůstává pro zbytek. To je
   jediný způsob, jak se dostat pod ~15 instrukcí na pár včetně volání.
5. **LIFO znovupoužití + bump z čerstvé stránky**: naposledy uvolněný blok
   je horký v L1; bump alokace ze sekvenční stránky má dokonalou prefetch
   predikci. Toto je zdroj wall-clock zisku, ne instrukce.
6. **Bulk uvolnění arén** pro front-end (tokeny, AST) a pro požadavek jako
   celek: `free` neexistuje, aréna se zahodí. Pro CLI navíc: na konci procesu
   se neuvolňuje nic, jen se zavolají destruktory.
7. **`realloc` in-place** ve stejné třídě; růst `Vec<Value>` po 16 B krocích
   pokrývá většinu případů bez kopie.
8. **Debug varianta** s kanárky a otrávením pro testy; release bez kontrol.

Náklady: 800–1500 řádků `unsafe` Rustu (asm nepotřeba), 2–3 týdny včetně
harnessu (stress testy s náhodnými velikostmi, celá e2e sada pod
ASan-ekvivalentem, fuzzing `realloc`/alignment), plus přepis parseru na
jedno vlákno (samostatná položka, sama o sobě užitečná). Rizika: chyby
alokátoru se u PHPStanu projeví jako náhodné pády nebo tiché poškození dat;
nutný trvalý vlastník kódu.

Očekávaný přínos proti mimalloc: 3–6 % času (~0,1–0,2 s cold). Proti glibc
−20 až −25 %. Přínos §5 (méně alokací) je odhadem 10–20 % času a sčítá se.

## 3. Assembler

Assembler rychlou cestu nezkrátí:

- Rust/C pro tvar „TLS load, index, pop, store“ generuje přesně tu sekvenci,
  kterou bys napsal ručně; ověřitelné na `cargo asm`/`objdump` mimalloc:
  `mi_malloc` fast path je ~15 instrukcí bez volání.
- Největší fixní náklad není v těle alokátoru, ale v tom, že Rust všechny
  alokace směruje přes symbol `__rust_alloc` → `GlobalAlloc::alloc`; rychlá
  cesta se **nedá inlinovat do volajícího** ani z Rustu, ani z asm. Jediná
  cesta kolem je nevolat globální alokátor na horkých místech (vlastní pooly
  pro `Rc<String>`, argumentové buffery, arény), což je opět politika.
- Přenositelnost: repo cílí i na Apple/aarch64 (`target_vendor = "apple"`
  cfg), tedy dvě asm implementace + fallback.
- Inline asm je `unsafe` blok; `scripts/unsafe-baseline.env` má strop bloků
  1627 a funkcí 289 nastaven přesně, zvýšení vyžaduje samostatnou
  bezpečnostní revizi. Auditovat asm je řádově dražší než Rust.

Kde má ruční SIMD/asm v RPHP smysl obecně: kernely nad daty (SHA-512 pharu už
běží přes AVX2 v crate `sha2`, vyhledávání bajtů přes `memchr`), případně
skener lexeru. I tam stačí `std::arch` intrinsics; ruční asm nepřidá nic než
riziko.

## 4. Kdybychom ho přesto psali: návrh

- `#[global_allocator]` implementující `GlobalAlloc`; bloky > 1 KB nebo se
  zarovnáním > 16 předat `mmap`/systémovému malloc.
- Size classes šité na míru: 16, 32, 40, 48, 56, 64, 80, 96, 112, 128, 152,
  192, 256, 384, 512, 768, 1024 B.
- Segmenty 2–4 MB z `mmap`, stránky 64 KB, jedna třída na stránku, hlavička
  stránky na začátku (size class, vlastnické vlákno, počet použitých,
  lokální free list, atomický odložený list).
- Per-thread heap: pole aktuálních stránek a free listů podle třídy; `alloc` =
  pop, při prázdném listu bump z aktuální stránky, při plné stránce převzít
  odložené `free` cizích vláken, jinak nová stránka.
- `free`: adresa & !(64 KB − 1) → hlavička → pokud vlastník == aktuální vlákno
  push do lokálního listu, jinak CAS push do odloženého listu.
- `realloc`: in-place, pokud nová velikost padne do stejné třídy.
- Vracení OS: `madvise(MADV_DONTNEED)` na plně volné stránky, lazy; pro CLI
  prakticky nepotřebné.
- Debug režim: kanárky, detekce double-free, otrávení uvolněných bloků.

Rozsah: 600–1200 řádků, ~80 % `unsafe` (odhad 40–60 nových unsafe bloků a
funkcí). Testy: kontrakty `GlobalAlloc` (alignment, zeroed, realloc), stress
test s náhodnými velikostmi a cross-thread free, spouštění celé e2e sady pod
ASan-ekvivalentem. Miri přes `mmap` neprojde, nutný vlastní harness.

Odhad: 1–2 týdny do důvěryhodné verze; první týden odhalí chyby, které se u
PHPStanu projeví jako náhodné pády nebo tiché poškození dat.

Očekávaný výsledek: 3,5–3,7 s cold (mimalloc 3,68 s), tj. v nejlepším případě
−5 % proti hotovému řešení, za cenu trvalé údržby nejcitlivějšího kódu v
procesu a konfliktu s unsafe policy.

## 5. Kde je skutečný výnos: alokovat méně

Každá z těchto položek je bezpečný Rust a měřitelná po krocích:

| opatření | ušetřené alokace / běh (odhad) | poznámka |
|---|---|---|
| jednoalokační řetězce (`Rc<str>`-style layout `Value` string) | ~1/3 všech alokací (2 → 1 na řetězec) | největší položka; zásah do reprezentace `Value`, `SharedStringKey`, ABI `Rc<String>` sdíleného s klíči polí a cache |
| aréna pro tokeny + AST jednoho souboru | ~1,5 M `free` a stejně `alloc` | uvolnit jedním tahem po kompilaci; vyžaduje `Vec<Token>`/`Box<Expr>` nad arénou |
| recyklace `Vec<Value>` argumentových bufferů | stovky tisíc | částečně existuje (`recycle_function_argument_buffer`) |
| bez klonů `Value` v `compare_inner` (`dereferenced().clone()` obou operandů) | ~2 M `Rc` inc/dec | ne alokace, ale stejná kategorie |
| `packed_from_values` pro 3-prvková pole tokenů: inline storage pro ≤ 3 prvky | ~700 k na `token_get_all` | `SmallHash` má inline kapacitu 3 už pro hash; packed ne |

Souhrn: snížení počtu alokací o 30–40 % dá stejný nebo větší wall-clock
efekt jako přechod glibc → mimalloc, a sčítá se s ním.

## 6. Doporučení (při cíli „absolutně nejrychlejší alokace“)

1. Hned: zmergovat mimalloc (−0,95 s cold). Slouží jako referenční laťka,
   kterou vlastní alokátor musí měřitelně překonat, jinak nemá smysl.
2. Před psaním alokátoru udělat §5 (méně alokací) a přepis parseru na jedno
   vlákno: oboje zvyšuje výnos alokátoru (méně párů, thread-local bez
   atomik) a má vlastní návratnost.
3. Vlastní alokátor psát v Rustu podle §2b/§4, s typovanými inline pooly na
   horkých místech; assembler nepoužít — rychlá cesta je už pětiinstrukční
   a kompilátor ji generuje sám. Cíl: ≤ 0,6 G instrukcí v alokaci, LIFO
   lokalita, 3–6 % času proti mimalloc; ověřovat callgrind + wall na
   PHPStanu po každém kroku.
4. Pořadí implementace pro rychlou zpětnou vazbu: (a) thread-local
   size-class pool jako `GlobalAlloc` s fallbackem na mimalloc pro velké
   bloky a cizí vlákna → změřit; (b) přímé pooly pro `Rc<String>`/`Rc<PhpArray>`
   → změřit; (c) arény front-endu → změřit. Každý krok samostatně obhajitelný.

## 7. Histogram alokací cold běhu (měřicí GlobalAlloc wrapper, main 3e1f2580)

Zdroj: `src/alloc_stats.rs` na větvi `codex/php-heap-measure` (wrapper nad
`System`, evidence bloků v mmap tabulce; `RPHP_ALLOC_STATS=1`). Instrumentace
sama běh zpomaluje, čísla jsou počty, ne časy.

- **24,6 M alokací, 21,0 M uvolnění, 0,9 M realloc**, celkem 3,97 GB
  alokovaných bajtů; peak live 584 MB ve 4,08 M blocích.
- **99,1 % bloků ≤ 1 KB; 69 % ≤ 48 B; 96 % ≤ 192 B.** Velké bloky (> 64 KB)
  jsou stovky za běh.
- Nejčastější velikosti: 40 B (10,4 %, hlavička `RcBox<String>` = 2,55 M
  řetězců), 24 B (8,5 %), 48 B (6,7 %, tříprvkový `Vec<Value>`, tokeny),
  152 B (5,3 %, `RcBox<PhpArray>`), 1–24 B bytové buffery řetězců (~35 %
  všech alokací dohromady), 84 B (2,0 %), 104 B (1,7 %), 128 B (1,6 %).
- Zarovnání: 47 % align 1 (bajty řetězců), 46 % align 8, 4,5 % align 16
  (`Vec<Value>`), nic nad 16 kromě dvou stránkových bloků.
- **Životnost: 19,6 % bloků zanikne před další alokací, 49 % do 2 dalších
  alokací, 59 % do 4, ~75 % do 64.** Přibližně 15 % žije přes 2^10 alokací
  (op arrays, třídy, kontejner).
- Vlákna: hlavní vlákno 23,5 M alokací, parser 1,16 M (4,7 %); **cross-thread
  free 608 k (2,5 %)** — AST a tokeny alokované parserem, uvolněné hlavním
  vláknem.
- Realloc: 915 k, z toho 78 % roste o více než jednu 16 B třídu (amortizovaný
  růst `Vec`/`String`), 3,8 % zůstává ve stejné třídě.

Důsledky pro návrh: LIFO free list per třída dává téměř dokonalou lokalitu
(blok uvolněný před okamžikem je ještě v L1); třídy na míru: 8, 16, 24, 32,
40, 48, 56, 64, 80, 96, 112, 128, 152, 168, 192, 256, 384, 512, 768, 1024
(pokryjí 99 %); align ≤ 16 stačí pro celý horký provoz; cross-thread free je
řádově procenta a patří na oddělenou pomalou cestu; polovina alokací jsou
bytové buffery řetězců, které inline hlavička sloučí s `RcBox` do jedné
alokace (2,55 M alokací méně, tj. ~10 %).

Surový výstup:

```
[alloc] allocs=24638580 frees=20977787 reallocs=915125 zeroed=337690 total_bytes=3973 MB peak_live=584 MB peak_blocks=4079864 cross_thread_frees=608041 unknown_frees=18726 table_drops=0
[alloc] realloc: same16=34451 grow_next16=167110 grow_other=711978 shrink=1586
[alloc] allocs by thread: t0=0 t1=23470814 t2=859 t3=668 t4=191 t5=1307 t6=587 t7=1164154
[alloc] sizes <=1024: 24410601 (99.1%)
[alloc]   size    40:    2554333 (10.37%)
[alloc]   size    24:    2102491 ( 8.53%)
[alloc]   size    48:    1650346 ( 6.70%)
[alloc]   size   152:    1304605 ( 5.29%)
[alloc]   size     4:     976204 ( 3.96%)
[alloc]   size    32:     967939 ( 3.93%)
[alloc]   size    64:     878289 ( 3.56%)
[alloc]   size    16:     592340 ( 2.40%)
[alloc]   size     8:     539194 ( 2.19%)
[alloc]   size    21:     498802 ( 2.02%)
[alloc]   size    84:     493185 ( 2.00%)
[alloc]   size     5:     469279 ( 1.90%)
[alloc]   size     7:     442668 ( 1.80%)
[alloc]   size   104:     414736 ( 1.68%)
[alloc]   size   128:     385335 ( 1.56%)
[alloc]   size     6:     384195 ( 1.56%)
[alloc]   size    12:     334272 ( 1.36%)
[alloc]   size    41:     326502 ( 1.33%)
[alloc]   size    10:     324126 ( 1.32%)
[alloc]   size    44:     313353 ( 1.27%)
[alloc]   size     3:     297133 ( 1.21%)
[alloc]   size    52:     294989 ( 1.20%)
[alloc]   size     9:     254144 ( 1.03%)
[alloc]   size    11:     237249 ( 0.96%)
[alloc]   size    15:     219847 ( 0.89%)
[alloc]   size     2:     217325 ( 0.88%)
[alloc]   size    72:     208211 ( 0.85%)
[alloc]   size    20:     205672 ( 0.83%)
[alloc]   size     1:     204688 ( 0.83%)
[alloc]   size    18:     190118 ( 0.77%)
[alloc]   size    17:     188862 ( 0.77%)
[alloc]   size    14:     183659 ( 0.75%)
[alloc]   size    96:     177689 ( 0.72%)
[alloc]   size    50:     167214 ( 0.68%)
[alloc]   size   168:     167010 ( 0.68%)
[alloc]   size    13:     163375 ( 0.66%)
[alloc]   size    56:     163032 ( 0.66%)
[alloc]   size    36:     150123 ( 0.61%)
[alloc]   size    19:     147533 ( 0.60%)
[alloc]   size    38:     146532 ( 0.59%)
[alloc]   <=    16 B:  23.7%
[alloc]   <=    32 B:  44.4%
[alloc]   <=    48 B:  69.1%
[alloc]   <=    64 B:  78.2%
[alloc]   <=    96 B:  84.0%
[alloc]   <=   128 B:  88.6%
[alloc]   <=   192 B:  96.2%
[alloc]   <=   256 B:  96.9%
[alloc]   <=   512 B:  98.2%
[alloc]   <=  1024 B:  99.1%
[alloc]   large < 2^11:    108390 ( 0.44%)
[alloc]   large < 2^12:     40850 ( 0.17%)
[alloc]   large < 2^13:     27919 ( 0.11%)
[alloc]   large < 2^14:     20064 ( 0.08%)
[alloc]   large < 2^15:     20015 ( 0.08%)
[alloc]   large < 2^16:      8955 ( 0.04%)
[alloc]   large < 2^17:       908 ( 0.00%)
[alloc]   large < 2^18:       417 ( 0.00%)
[alloc]   large < 2^19:       230 ( 0.00%)
[alloc]   large < 2^20:       128 ( 0.00%)
[alloc]   large < 2^21:        34 ( 0.00%)
[alloc]   large < 2^22:        21 ( 0.00%)
[alloc]   large < 2^23:        19 ( 0.00%)
[alloc]   large < 2^24:        13 ( 0.00%)
[alloc]   large < 2^25:        13 ( 0.00%)
[alloc]   large < 2^26:         2 ( 0.00%)
[alloc]   large < 2^28:         1 ( 0.00%)
[alloc]   align     1:   11624000 ( 47.2%)
[alloc]   align     2:     127522 (  0.5%)
[alloc]   align     4:     500495 (  2.0%)
[alloc]   align     8:   11270116 ( 45.7%)
[alloc]   align    16:    1116445 (  4.5%)
[alloc]   align  4096:          2 (  0.0%)
[alloc]   lifetime < 2^ 1 allocs:   4122063 ( 19.6%)
[alloc]   lifetime < 2^ 2 allocs:   6144329 ( 29.3%)
[alloc]   lifetime < 2^ 3 allocs:   2149257 ( 10.2%)
[alloc]   lifetime < 2^ 4 allocs:   1128931 (  5.4%)
[alloc]   lifetime < 2^ 5 allocs:    557335 (  2.7%)
[alloc]   lifetime < 2^ 6 allocs:    662263 (  3.2%)
[alloc]   lifetime < 2^ 7 allocs:    400726 (  1.9%)
[alloc]   lifetime < 2^ 8 allocs:    325043 (  1.5%)
[alloc]   lifetime < 2^ 9 allocs:    325272 (  1.6%)
[alloc]   lifetime < 2^10 allocs:    378369 (  1.8%)
[alloc]   lifetime < 2^11 allocs:    474440 (  2.3%)
[alloc]   lifetime < 2^12 allocs:    571182 (  2.7%)
[alloc]   lifetime < 2^13 allocs:    646512 (  3.1%)
[alloc]   lifetime < 2^14 allocs:    361351 (  1.7%)
[alloc]   lifetime < 2^15 allocs:    256379 (  1.2%)
[alloc]   lifetime < 2^16 allocs:    240757 (  1.1%)
[alloc]   lifetime < 2^17 allocs:    223446 (  1.1%)
[alloc]   lifetime < 2^18 allocs:    307482 (  1.5%)
[alloc]   lifetime < 2^19 allocs:    334277 (  1.6%)
[alloc]   lifetime < 2^20 allocs:    408002 (  1.9%)
[alloc]   lifetime < 2^21 allocs:    367562 (  1.8%)
[alloc]   lifetime < 2^22 allocs:    266504 (  1.3%)
[alloc]   lifetime < 2^23 allocs:    131058 (  0.6%)
[alloc]   lifetime < 2^24 allocs:    175795 (  0.8%)
[alloc]   lifetime < 2^25 allocs:       726 (  0.0%)
```
