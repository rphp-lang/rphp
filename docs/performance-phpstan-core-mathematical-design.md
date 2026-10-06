# Matematický návrh nového jádra pro tři oblasti PHPStanu

Stav: návrh před realizací. Výchozí verze je `d7723d96`, ordinary release,
default features, bez PGO. Jádro ani PHPStan se tímto návrhem nemění.
Cíl 100× není doložený výsledek ani předpoklad, který smí nahradit měření.

## 1. Přesná definice cíle

Požadujeme pro každou oblast j současně `I'_j <= I_j / 100` a
`T'_j <= T_j / 100`, stejný vstup, skutečnou analýzu, shodné diagnostiky,
exit a PHP sémantiku. Všechny výsledky musí patřit jedné přesné verzi zdrojů.
Základní instrukční rozpočty vycházejí z konečného ověřeného executable;
časové hodnoty jsou jedna atribuční iterace, nikoli přesné fyzikální konstanty.
Prázdná instrumentace je měřena zvlášť a odečtena. GC je v těchto původních
oblastech vypnutý v obou interpretech; varianta se zapnutým GC je samostatná.
Číselné rozpočty jsou pro současný x86-64 host. ARM64 má tentýž sémantický návrh,
ale potřebuje vlastní reference/baseline; počet ISA instrukcí mezi hosty
nepřenášíme jako stejnou jednotkovou cenu.

| Oblast | Výchozí instrukce | Rozpočet 100× | Výchozí čas | Cílový čas |
| --- | ---: | ---: | ---: | ---: |
| Nativní funkční reflexe | 14 351 797 067 | 143 517 970 | 1,140027 s | 11,400 ms |
| Průniky mimo nativní reflexi | 13 996 737 129 | 139 967 371 | 1,319012 s | 13,190 ms |
| Stuby mimo ostatní sledované oblasti | 13 243 893 100 | 132 438 931 | 0,935074 s | 9,351 ms |

Sledovaná reflexe má 96 vstupů do poskytovatele včetně jeho potomků; průniky
17 233 volání a 9 343 vnějších měřených intervalů; stuby 67 intervalů.
Amortizovaný rozpočet je přibližně 1,495 milionu instrukcí na vstup reflexe,
8 122 na započtené volání průniku a 1,977 milionu na interval stubů.
To nejsou ceny jednoho primitivního lookupu, porovnání typu ani načtení souboru.

Průniky běží v PHP algoritmu `TypeCombinator::doIntersect`; parser stubů je
parser napsaný v PHP. Reflexní interval obsahuje práci PHP poskytovatele.
Optimalizace samotného Rust parseru nebo vestavěného ReflectionFunction proto
nemůže být bez dalšího označena za přepis těchto celých oblastí. Jejich názvy
identifikují měření, nikdy podmínky v RPHP.

Cíl navíc odpovídá 11,7–15,6× menšímu počtu instrukcí než dnešní referenční PHP
v těchto oblastech. Nestačí odstranit sedminásobný rozdíl proti PHP.
Tabulka používá floor instrukčního limitu; do přesného neceločíselného podílu
se nesmí zaokrouhlením přijmout větší celočíselný výsledek.

## 2. Účetnictví práce, nikoli názvů funkcí

Rozdělíme skutečně vykonané instrukce nové implementace do nepřekrývajících se
kategorií:

`I'_j = C_j + W_j + L_j + A_j + D_j + G_j + F_j + M_j`.

- C: sestavení společného IR, analýza efektů, registrů a native kódu;
- W: nutné výpočty nad vstupy a výsledky;
- L: lookup deklarací, klíčů, tvarů a metadat;
- A: skutečné alokace a inicializace pozorovatelných hodnot;
- D: skutečné uvolnění vlastníků a infrastruktura GC/destrukce;
- G: vstupní a vnitřní guardy;
- F: dodatečné řízení side exitu po neúspěšném guardu;
- M: materializace stavu, vlastníků a logických PHP aktivací.

Cena potomků se započítá právě jednou. Inkluzivní profily nejsou položkami této
sumy. Užitečná práce, callback a alokace se nestanou odstranitelnými tím, že
leží pod DoFcall nebo Return. Koeficienty nevyplňujeme odhadem z cizího buildu.
Po neúspěšném guardu patří skutečná práce canonical pokračování opět do W/L/A/D,
nikoli podruhé do F. F zahrnuje jen řízení side exitu; rekonstrukce patří do M.
Tělo PHP destruktoru se stejně jako jiný callback rozdělí do W/L/A/D/G/F/M;
jeho celá inkluzivní cena se nepřidá k D. Jedna fused instrukce má v účtu jednu
cenu, i když plní několik sémantických požadavků.

Pro konkrétní transformaci lze odvodit dolní mez `LB_j` podle nutných
čtení/zápisů, identity výsledků, callbacků a efektů. Pokud ověřená `LB_j`
překročí rozpočet, tato transformace nemůže splnit 100×. Dnešní počty volání
samy tuto mez neurčují: getter lze inlinovat či odstranit společným výpočtem,
zatímco čtení vstupu a pozorovatelný zápis musí zůstat. Výstupní graf se nesmí
zaměnit za jeden vrácený handle jen proto, že výpis diagnostik zůstane stejný.

## 3. Podmínka dosažitelnosti a cena přípravy

Nechť f je podíl dnešních instrukcí opravdu nahrazených novou cestou, s její
zlevnění a g nový dodatečný náklad jako podíl celého starého rozpočtu. Potom:

`I' / I = (1 - f) + f / s + g`.

Pro 100× tedy musí platit `f > 0,99 + g` a
`s >= f / (f - 0,99 - g)`. Při rovnosti je konečné s nedostačující.

| Pokrytí dnešního nákladu | Zlevnění pokryté části | Celkové zrychlení bez nového nákladu |
| --- | ---: | ---: |
| 90 % | 100× | 9,17× |
| 99 % | 100× | 50,25× |
| 99 % | 1 000× | 90,99× |
| 99,5 % | 200× | 100,25× |

Ilustrativních 10 milionů nových instrukcí přípravy při pokrytí 99,5 % zvýší
potřebné zlevnění pokryté části na 231,22× v reflexi, 232,18× v průnicích a
234,40× ve stubech. Těch 10 milionů je parametr citlivostní analýzy, nikoli
změřená cena existujícího kompilátoru. Pokrytí znamená podíl skutečných
instrukcí, nikoli počet podporovaných opcodů nebo podíl nalezených metod.

Instrukce nejsou čas. Samostatně platí model dolní meze:

`T' >= max(I' / throughput, přenesené_bytes / bandwidth,
          kritická_latence, nepřekryté_čekání)`.

CPU cena přípravy a callbacků už je v I'; nepřičítá se podruhé. Sériové fáze
mají vlastní meze a jejich skutečné časy se sčítají. Throughput a latency nejsou
konstanty společné všem programům. SIMD nebo více cache missů může zmenšit počet
instrukcí bez úměrného zmenšení času. Obě podmínky proto ověřujeme odděleně.

## 4. Zvolená architektura: jedno sémantické IR a placené hranice efektů

Canonical VM definuje chování. Kompilátor z jejího CFG vytvoří společné typed
SSA IR, v němž jsou odděleny výpočty, vlastníci a pozorovatelné efekty. Stejný
IR používá ověřovací executor a obě native architektury. Nerozšiřujeme několik
nezávislých rozpoznávačů PHPStanových tvarů.

Reprezentace každé operace obsahuje definované vstupy, výstup, efektový token,
provedená fakta, vlastnický přenos a přesné místo pokračování. Rozlišujeme:

`Compute, ReadMetadata, ReadHeap, WriteHeap, Allocate, Retire,
Call, ObserveFrame, EmitDiagnostic, Throw, Suspend, Poll`.

Pure výpočty s doloženými závislostmi lze spojovat, eliminovat a inlinovat.
Vzájemné pořadí pozorovatelných efektů zůstává zachované. Neznámý efekt je
hranice, přes kterou optimalizace nepřejde. Ticks, signal polling, reflexe
aktivního rámce, variadické originální argumenty a PHP diagnostiky jsou efekty.

Efekt sám neznamená povinný návrat do bytecode dispatch. Doloženou efektovou
operaci může native kód vykonat přes společné sémantické primitivum a pokračovat
za ní s novým efektovým tokenem. Před callbackem se zveřejní potřební vlastníci
a logické aktivace. Neznámá operace použije canonical helper/pokračování; nesmí
se přes ni přenést staré aliasové či metadatové předpoklady. Tím oddělujeme
zachování efektu od drahého návratu pro každou jednotlivou PHP instrukci.

Algoritmus převodu:

1. Vytvořit CFG včetně výjimečných vstupů, catch/finally a resumption hranic.
2. Spočítat def/use, SSA, aliasové a efektové souhrny do pevného bodu. Neznámé
   callee efekty zůstávají neznámé; analýza nepovažuje metodu za pure podle názvu.
3. Odvodit typ/tvar/identitu deklarace a potřebu skutečných vlastníků.
4. Propagovat konstanty a doložená fakta, odstranit mrtvé pure výsledky a
   provést dominátorové CSE pouze nad stejnými hodnotami a efektovými verzemi.
5. Inlinovat doložené malé tělo, když celková cena včetně guardu a code growth
   vyjde nižší. Rekurze a nevyřešená dynamická vazba nedostanou nekonečný plán.
6. Vybrat nejlevnější pokrytí bloků, přidělit registry podle živosti a vytvořit
   stack maps pro všechny guardy, výjimky a efekty.
7. Lowerovat tentýž IR do checked executoru a ARM64/x86-64; každá neprovedená
   operace má původní baseline pokračování bez replay již provedeného efektu.

### Lookup a metadatová rovina

Nezměněné deklarace používají FunctionId, ClassId, ShapeId, scope a epochu
registry. Literální názvy se řeší jednou, kladné immutable vazby se uchovají.
Negativní lookup nelze zafixovat přes autoload, alias či změnu registry.
Metadata parametrů a návratových typů jsou sdílené immutable deskriptory;
pozorovatelné reflexní objekty mají při požadovaném vytvoření čerstvou identitu.

Cíl lookupu: `O(délka_názvu)` při skutečném prvním rozlišení, následně `O(1)`
po ověření příslušných epoch. Výčet P parametrů stojí nejméně `O(P)` pokud
uživatel skutečně pozoruje P položek. Připravit více tabulek při každém getteru
se do návrhu nehodí. Guard musí ověřit binding i runtime scope.

### Výpočty typů a obecné opakované pure výpočty

Jádro může zlevnit své vlastní PHP type checks přes již rozlišené deklarace,
bitové scalar masky a immutable ancestry. To není nativní implementace
PHPStanového průniku. Jeho PHP smyčky musí využít společný IR, inlining,
specializované array/property operace a případně prokázanou memoizaci.

Memoizace má cenu `N * hit_cost + U * miss_cost`, kde N je počet požadavků a U
počet jedinečných sémantických vstupů; klíč zahrnuje všechny relevantní vstupy,
scope, deklaraci a verze čteného stavu. Povoluje ji efektový důkaz, ne jméno
funkce. Začne jen neheapovými scalar výsledky doložených pure výpočtů.
Pouhá identita mutable objektu není platný klíč. Cache nesmí přidat vlastníka,
prodloužit život argumentu či vracet starý objekt místo nové identity.
Generační/weak identita brání použití starého záznamu po znovupoužití adresy.
Cena verzování zápisů, klíčů a invalidace patří do výsledku; plošné verzování
každého PHP zápisu bez doloženého přínosu není automaticky výhra.

Přesněji, při stejně drahém opakovaném pure výpočtu s cenou c je původní účet
`N*c`. Cache stojí `N*h + U*c + V + C`, kde h zahrnuje celý klíč a probe, V
verzování/invalidaci a C přípravu. U počítá skutečné recomputations včetně
invalidovaných záznamů, nikoli jen počet různých adres. Poměr cen je
`U/N + h/c + (V+C)/(N*c)`. Samotná cache dosáhne 100× pouze pokud tento součet
nepřekročí 0,01. Hluboké hashování objektového grafu není O(1) klíč; i jeho cenu
je nutné zaplatit. Při levnějším IR miss výpočtu se člen U/N násobí poměrem
nové a původní ceny tohoto výpočtu. Vražení cache před libovolné volání není
algoritmus, který by dokazoval požadované zlevnění.

Native společný IR může provádět typový/property/table loop bez dispatch,
argument frame a Return pro každý malý getter. Operace se nesmí zahodit,
pokud může číst mutable alias, vyvolat magic, varování, hook či callback.

### Parser a tabulkové algoritmy

Pro smyčku napsanou v libovolném PHP: `O(B + T + R + A)` je rozpočtový model
pro nutné přečtené bytes, tokeny, skutečné redukce a materializované výstupy.
To není důkaz, že každý uživatelský parser je lineární. IR zlevní skutečnou
smyčku a stabilní lookupy; nemění automaticky aplikací zvolený algoritmus.
Příznak dense packed-array a shape guard umožní přímé indexed loads; mutable
reference/COW, chybějící index a ArrayAccess ponechají vlastní semantic cestu.
Nepřevádíme libovolný PHP parser na Rust parser podle jeho jména.

AST objekty mohou zůstat virtuální pouze při důkazu přesné identity, absence
pozorovatelné lifetime události a zachování memory-limit/účtovacích efektů.
Neprokázané alokace zůstanou skutečné. Pozorovaný výstupní graf i aliasy se
materializují s původními typy, identitami a pořadím. Sdílení cached AST při
mutovatelných objektech není povolené odstranění práce.

### Vlastníci, rámce a uvolnění

Uvnitř doložené oblasti reprezentuje SSA skutečnou živost; Value slot, bitmapa
ani argument frame se nevytvářejí jen kvůli předání pure mezivýsledku. Živost
se počítá podle používaných hodnot, nikoli celého historického rozsahu TMP.

Každý callback, exception, suspend a pozorování rámce dostane předem úplné
vlastníky a logické PHP aktivace. Běžné argumenty zůstávají vlastníky. Neexistuje
nekontrolované půjčení heap argumentu přes PHP volání. Inlining smí odstranit
fyzický rámec, nikoli callee scope, argumenty, trace nebo destrukční hranici.

Jeden retirement primitivum odpojí skutečného vlastníka. Shared edge se uvolní
s požadovanou GC rolí; při final ownership se projdou skutečné děti v PHP
pořadí, spustí callback s možností resurrection a zpracují výjimky. Iterativní
worklist omezuje hloubku. Neexistují dva nezávislé předprůchody grafu plus další
host drop. Předchozí malý retirement prototyp s asi 2% změnou není důkaz této
celkové architektury ani potřebného 100× efektu.

### Konkrétní odstranění mezikroků

Pro dvě čtení stejného slotu a opakované volání doložené pure malé metody:

```text
baseline: resolve; load; publish args; call; return; retire temporary
          resolve; load; publish args; call; return; retire temporary
IR:       guard binding/shape/scope; load; inline compute; use result twice
```

Transformace je platná jen při stejné verzi slotu, stejných vstupech a bez
mezilehlého efektu. Druhé vyřešení a výpočet zmizí; logické vlastnické a frame
události zůstávají tam, kde by je PHP mohlo pozorovat. Její cena se porovnává
se skutečně odstraněnými instrukcemi, nikoli s celou inkluzivní cenou volání.

U stabilní dense tabulkové smyčky je výstupem jeden guard rozsahu/tvaru,
indexed load a operace nad register hodnotami na iteraci. Není tam nový VM
dispatch, name lookup, frame ani retain/drop čistě interního scalar TMP na
každý krok. Jakmile zápis či callback může měnit závislost, musí zůstat kontrola
nebo oblast skončí. Nevyužitý výsledek observable volání se nikdy neeliminuje
jen podle def/use.

### Důkaz ekvivalence

Stav canonical VM zahrnuje heap, skutečné vlastníky, reference, COW, logické
rámce/argumenty, handlers, GC a PHP paměťové účetnictví. Pro každý IR bod
existuje rekonstrukční mapa R do tohoto stavu. Pure IR kroky buď odpovídají
canonical krokům, nebo jsou doloženě redundantní. Každý efekt musí vytvořit
stejnou pozorovatelnou událost a oba následující stavy opět splňují R.
Při side exitu se pokračuje z R za posledním již provedeným efektem.

Pozorovatelný sled obsahuje i warnings, autoload, hooks, argument/frame
introspection, destruktory, weak identity, GC, výjimky a pořadí finally.
Toto je lokální simulační závazek každé transformace; shodný JSON PHPStanu
sám není jeho důkaz. Není-li doložené logical allocation účetnictví včetně
memory_limit a memory_get_usage, heap virtualizace není připuštěna. První
realizace proto může odstraňovat interní scalar/slot/frame režii bez slibu
odstranění pozorovatelných AST či typových objektů.

### Celková cena přípravy IR

Souhrn se vytváří jednou pro identitu deklarace/bytecode a sémantické epochy,
nikoli znovu na každém volání. Call-site uchovává jen svoje guardy a vazby;
sdílený callee kód a souhrn zůstávají společné. Inline expansion dostává rozpočet
celkové velikosti, nikoli volnou rekurzi do dalších 24 tisíc metod. Analýza
aliasů/efektů má mez práce; po jejím vyčerpání zůstane unknown, nikoli domnělý
důkaz. Cena odmítnutého kandidáta se také započítá.

Užitečný rozklad compiler nákladu je:
`C = c_scan*V + c_edges*E + C_proof + C_lower + C_cache`, kde V/E jsou skutečně
navštívené IR body/hrany. Tento účet je podmínka admission; neprokazuje lineární
složitost celé analýzy pevného bodu. Reuse prokazatelně stejného souhrnu snižuje
V/E/C_proof, nikoli jen čas přemístěním mimo region. Pro cold jednorázová těla
zůstává levný canonical executor; do native se vybírá cesta, které zaplatí
přípravu skutečné opakované používání. Pokud tato politika nechá přes 1 % staré
instrukční ceny, není splněn 100× cíl bez dalšího odstranění skutečné práce.

## 5. Matematický výběr nejlevnější realizace

Pro region r a variantu v definujeme n návštěv, q podíl neúspěšných guardů,
h úplnou cenu úspěšné návštěvy, s cenu prefixu neúspěšné návštěvy včetně guardů,
materializace a side exitu a b cenu zbývajícího canonical pokračování:

`cost(r,v) = C(r,v) + n * [(1-q)*h + q*(s+b)]`.

C obsahuje compiler, důkazy, profiling, cache správu a invalidaci nezahrnutou
v ceně návštěvy. h/s/b se mohou lišit podle vstupu; potom se místo konstant
použijí součty cen skutečných cest. b nezačíná znovu před již provedeným efektem.

Počet návštěv a pravděpodobnost selhání jsou parametry pozorované obecně, bez
benchmarkových názvů. Volíme varianty minimalizující součet ceny při omezení
code bytes, cache bytes a přesného sémantického pokrytí. Každá nutná operace
a efekt jsou pokryty právě jednou, žádný výsledek se po fallbacku nepočítá znovu.

Formální zadání: minimalizovat celkové instrukce skutečné analýzy při omezeních
`I'_j <= B_j`, `T'_j <= H_j`, semantic equivalence a limitech code/cache/peak
memory. B/H jsou tři rozpočty z oddílu 1. Při stejné instrukční ceně rozhoduje
nižší čas a potom nižší cold/code cena. Pokud množina splňující 100× omezení
zůstane prázdná nebo nedoložená, návrh se za 100× řešení neoznačí. Lokální výběr
podle samotného času nesmí koupit rychlost větším počtem instrukcí.

V straight-line bloku lze volit disjunktní doložené úseky dynamickým programem:

`D[i] = min(D[i+1] + baseline_cost[i], min_{r začíná i}(cost[r] + D[end(r)]))`.

U smyčky se runtime složka násobí počtem iterací, vstupní guard se smí hoistovat
jen když závislosti zůstávají stabilní. Pro n instrukcí a K validních kandidátů
se už ověřený intervalový výběr provede v `O(n + K)` po seskupení podle začátku;
obsahuje-li stav resource limity, použije se Pareto frontier instrukce/čas/
code/cache velikost. Základní recurrence předpokládá už oceněné kandidáty bez
dalších sdílených nákladů a s doloženými vstupními/výstupními register stavy.
Společná kompilace se v celkovém účtu platí jen jednou; její závislost na výběru
několika regionů nelze vydávat za nezávislou cenu každého intervalu.
To není tvrzení, že tvorba kandidátů, CFG důkaz nebo globální register allocation
mají stejnou složitost. Přesný globální optimum tohoto návrhu není prokázáno.

Pro `delta = baseline_per_visit - [(1-q)*h + q*(s+b)] > 0` nastane break-even při

`visits >= ceil(C/delta)`.

Při rovnosti nákladů ještě nevznikla úspora; první striktně výhodná celočíselná
četnost je `floor(C/delta)+1`. Zvyšovat pouze pevný hot
threshold nebo přesouvat drahou přípravu mimo měřený interval není zlevnění.
Do účtu se zapíše i startup a teardown. Platný cached plán obsahuje plný důkaz,
identitu zdroje/deklarace a epochy, nikoli rozpoznané jméno aplikace.

Podmínku lokální výhodnosti nelze zaměnit za 100× celkové pokrytí: levný region,
který odstraní jen 1 % dnešního nákladu, nevyřeší zbývajících 99 %. Cold
jednorázová těla se nesmějí draze zkompilovat kvůli tomu, že jsou dlouhá.
Plány musí umět využít společný validovaný callee souhrn a jeho kód místo
opakovaného vytváření stejného velkého inline těla pro mnoho call sites.

### Dolní mez a cena nutné práce

Cost sheet obsahuje počty nutných sémantických požadavků q: přečtené input
bytes, použité table buňky, scalar operace po CSE, skutečně vytvořené výstupní
uzly, vlastnické události a pozorovatelné efekty. Zvlášť uvádí reuse a guard
závislosti. Známých 96/17 233/67 je počet vstupů, nikoli tento vektor q.
Požadavek q musí mít důkaz nutnosti pro zvolenou třídu transformací. Každá
scalar operace zbývající po CSE ještě není automaticky neodstranitelná;
algebraické zjednodušení či dead-output důkaz může dále změnit q. Dolní mez
pro pevné q se proto nevydává za optimum všech myslitelných PHP programů.

Pro doloženou sadu lowered jednotek v definujeme dolní cenu c_v a pokrytí
a_kv požadavku k. c_v musí být platná dolní mez, nikoli průměr několika
měřených návštěv; skutečný horní účet má vlastní ceny a cesty. Relaxation je:

```text
LB = min sum_v c_v*x_v
     pro x_v >= 0 a sum_v a_kv*x_v >= q_k pro každé k.
```

Fused jednotka může zaplatit více požadavků jednou; součet nezávislých cen
jednotlivých PHP opcodů by toto nerespektoval. Po vypuštění pořadí, celočíselnosti,
aliasů a register constraints jde pouze o dolní mez. Musí zahrnout každou
přípustnou realizaci nebo platné dolní meze pro dosud neoceněnou třídu; prázdná
sada kandidátů není důkaz nemožnosti. Cenu přípravy a fallbacku lze ignorovat
při této dolní mezi, ale ne při výběru skutečné implementace.

Dual certificate tvoří váhy y_k >= 0 splňující
`sum_k a_kv*y_k <= c_v` pro všechny přípustné jednotky. Potom
`sum_k q_k*y_k <= I'` bez dvojího počítání jedné fused instrukce. Pokud už tato
prokázaná mez překročí B_j, další dispatch tuning nemůže požadavek splnit;
musí se doloženě snížit počet nutných požadavků q. Malá dolní mez naopak ještě
nedokazuje existenci legálního plánu pod rozpočtem.

Praktický horní účet používá už vybraný legální disjunktní plán, všechny jeho
skutečné cesty, compiler a side exity. Potřebujeme současně `LB_j <= B_j` a
horní účet `UB_j <= B_j`. Dokud q a jednotkové ceny nejsou doložené, existuje
matematické zadání a vybraná architektura, ne důkaz dosažitelnosti 100×.

## 6. Co je nutné prokázat před realizací a co znamená hotovo

Pro každou oblast musí první cost sheet obsahovat skutečné četnosti potřebných
read/write, výstupních hodnot, final-owner callbacků a logical activations;
četnost reuse, velikost input/output a cenu specifikace guardů. Aktuální počet
96/17 233/67 nestačí k zaplacení nebo eliminaci vnitřního výpočtu. Neznámé
položky zůstávají neznámé, nikoli nula. Proto zatím není prokázaná dosažitelnost
100× a žádná konkrétní coverage f nebyla připsána novému jádru.

Před prvním přepisem se uzavře návrh operací, proof/fallback kontrakt a cost
sheet s přiznanými intervaly neznámých cen. Realizace začne společným effect IR
a ověřovacím executorem; kalibruje jeho jednotkové náklady, doplní dosud
neznámá q a teprve potom loweruje tentýž model do native. Transformace se vybírá
podle kladné celkové úspory a skutečného pokrytí. Plán, jehož doložený horní účet
nevychází do všech tří rozpočtů, není 100× řešení. Žádný dílčí krok není
dokončením celého požadavku.

Akceptace zůstává: stejné regiony a skutečné cold analysis, phase i whole-process
účet, všechny validní vzorky, shodná pozorovatelná PHP sémantika, references/COW,
magic/lazy/hooks, args, GC/destrukce/resurrection, pending/replacement exceptions,
generators/Fibers, diagnostiky a exact fallback. Úplné feature a sanitizer brány,
unsafe ceiling 1 747/321 a samostatně popsaná obě native architektury zůstávají.

Samotné 100× zrychlení těchto tří bodů rovněž automaticky nedokazuje paritu
celého PHPStanu. Kdyby jejich oddělené ceny tvořily jednu disjunktní sumu,
pokles 41,592G na 0,416G by ponechal asi 33,521G celkové analýzy, tedy asi 2,23×
celkové zrychlení proti 74,697G. To je podmíněný Amdahlův výpočet: tři samostatně
instrumentované zdroje zatím nedokazují společné disjunktní účetnictví. Celkový
cíl instrukcí i času na úrovni PHP tak zůstává vlastní bránou.

Přesná data, cíle a citlivostní výpočty jsou v
[matematickém packetu](performance-phpstan-core-mathematical-design-data.json).
Původní zdroj a všechny aktuální gate výsledky jsou v
[obnoveném základu](performance-phpstan-recovered-foundation.md).
