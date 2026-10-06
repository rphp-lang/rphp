# Matematický návrh obecného PHP jádra

Stav: návrh před realizací. Výchozí jádro je `d7723d96`, se stejným runtime
otiskem jako dokumentační checkpoint `54da5c4c`. Rozsahem je celé PHP: jazyk,
deklarace, standardní knihovna a vazba na prostředí. PHPStan je jeden nákladový
svědek. Jeho tři měřené intervaly neurčují podporované operace ani admission.
Tento dokument nahrazuje architektonické vymezení předchozího
[návrhu tří intervalů](performance-phpstan-core-mathematical-design.md).

## 1. Univerzální zadání

Nechť `theta` určuje PHP verzi, integer/float konfiguraci, rozšíření, SAPI,
request nastavení a rozhraní prostředí pro cílovou PHP specifikaci. Rozsah
specifikace se neomezí podle toho, co současné RPHP již umí. `P_theta` obsahuje všechny PHP programy
pro tuto konfiguraci, včetně dynamického kódu, chybného zdroje, předčasného
ukončení a neukončujících běhů. Není omezeno na spuštěné benchmarky. Model není
tvrzením, že RPHP již všechny tyto programy správně implementuje.

Pro každé `p` požadujeme stejné jazykově pozorovatelné chování při stejných
přípustných odpovědích prostředí:

```text
forall theta, p in P_theta, initial state s, compatible world responses w:
    Obs(exec_new(p, s, theta, w)) = Obs(PHP_semantics(p, s, theta, w))
```

U neukončujícího běhu se rovnost vztahuje na každý konečný prefix. Zahrnuje
výstup, návraty, změny hodnot a aliasů, introspection, diagnostics, exceptions,
autoload/hook/handler/callback pořadí, suspend/resume, destrukci a ukončení.
Čas, náhoda a externí I/O jsou skutečné efekty; nememoizují se ani nenahrazují
umělou hodnotou. Rovnost při sdílených externích odpovědích neznamená identická
čísla dvou nezávisle běžících hodin nebo RNG.

Minimalizujeme funkci `I_A(p,s,theta,w)` přes legální obecné architektury A.
Má zahrnout startup, source load, překlad, plánování, vlastní vykonání, fallback,
GC a teardown. Čas `T_A` zůstává druhou podmínkou; instrukční minimum nesmí
vzniknout placením nepřijatelné latence či neomezené paměti/kódu/cache.
Neexistuje jeden známý mix programů, kterým by se směla nahradit univerzální
sémantická podmínka. Optimalizace se porovnávají parametrickými cenami a
ověřenými podmínkami pro jejich použití; globální nejmenší možný PHP engine
tímto návrhem není dokázán.

Rozsah odpovídá jazyku včetně reference/value modelu, dynamických deklarací,
objektů, generators a Fibers a dále efektovým kontraktům knihovny/prostředí.
Přehled požadované syntaxe a kategorií vychází z
[PHP Language Reference](https://www.php.net/manual/en/langref.php).
Současný stav implementace zůstává popsán v [compatibility status](compatibility.md).

## 2. Stav a kompoziční důkaz

Sémantický stav je
`S = (declarations, heap, references, frames, continuations, request, lifetime, world)`.

| Část | Co musí reprezentovat |
| --- | --- |
| declarations | Functions/classes/interfaces/traits/enums, namespace bindings, constants, signatures, type/attribute/source metadata a jejich publikaci |
| heap/references | PHP skaláry, byte strings, ordered arrays, objects, resources, alias cells, COW a stavy property/lazy/weak hodnot |
| frames | Argumenty, named/variadic/by-ref vazby, locals/dynamic symbols, globals/statics, closure captures, lexical i called-class scope |
| continuations | Branch/loop/foreach, call/return, catch/finally, pending exceptions, generator/Fiber a exit/shutdown stav |
| request | Error reporting/handlers, ticks/assertions, INI/locale/encoding nastavení a PHP-visible paměťové účetnictví |
| lifetime | Vlastníci, GC roots, weak vztahy, destructor/resurrection a resource finalization |
| world | Streams/wrappers, files, síť, procesy, extension state, hodiny, RNG a SAPI |

Sémantické primitivum má explicitní kontrakt
`step(op, inputs, S) -> (result, S', events, continuation)`.
Kontrakt uvádí read/write footprint, alias a ownership změny, diagnostics,
možné callbacks/suspension a přesný failure prefix. Zápis do heapu není pure;
čtení může mít efekt přes undef, magic, property hook, lazy initialization,
ArrayAccess nebo jiné jazykové rozhraní. Neznámý kontrakt se nepovažuje za pure.

Pro optimalizovaný stav existuje rekonstrukční relace R k PHP stavu. Každý
provedený efekt vytvoří stejnou událost a navazující stavy opět splní R.
Interní kroky lze sloučit/odstranit pouze při tomto důkazu. Fallback pokračuje
za posledním již provedeným efektem, bez replay. Z lokálních důkazů a zachování
sekvence/branch/loop/call/resume se skládá důkaz pro libovolný program.

Neomezená série interních kroků nesmí skrýt pending exit, signal/tick či
suspension. Poll/declaration/diagnostic body zůstávají součástí kontraktů.
Logický frame, owner a PHP paměťové účetnictví musí být dostupné před observerem.
Heap virtualizace bez důkazu identity, lifetime a účetnictví není připuštěna.

Canonical RPHP je první vykonávací cesta pro už správně implementované chování.
Chybějící PHP konstrukce nebo native handler nejsou vyřešeny slovem fallback:
jejich sémantický kontrakt i implementační/proof mezera se musí výslovně doplnit.
Tento návrh nerozšiřuje současnou compatibility certifikaci.

## 3. Úplný nákladový účet

Pro skutečný běh se všechny vykonané instrukce zaúčtují právě jednou:

```text
I_A = source_IO + compile/link + plan/cache
    + useful_compute + lookup + guards
    + observable_allocations + ownership/GC
    + materialization + side_exit_control + external_body + teardown
```

Callback/destructor/extension tělo se rozdělí do příslušných položek; jeho
inkluzivní cenu nepřidáváme podruhé. Canonical pokračování je skutečný lookup,
výpočet či allocation, nikoli ještě jedna inkluzivní fallback cena. Jedna
fused instrukce může pokrýt více sémantických požadavků, ale má jednu cenu.

Parametry každého běhu jsou zejména potřebné bytes, jedinečné deklarace,
skutečné name resolutions, dotazy na metadata, navštívené typové uzly/hrany,
vyrobené výstupy, owner události, callbacks a GC průchody. Počítají se i
compiler/proof/invalidace a chybná či odmítnutá cesta. Jednotková cena je funkce
velikosti a sémantického stavu; složitý string/array/extension call nedostane
konstantní cenu jen proto, že jde o jeden opcode.

Pro každý kernel uvádíme `Gamma`, za kterých jeho horní účet platí, cenu guardu
Gamma a přesnou cenu canonical cesty, když Gamma selže. Vazby jsou na fakta
o hodnotách, deklaracích a efektech. Jména projektů ani knihoven nejsou Gamma.
Čas má vlastní latency/bandwidth/I/O účet a měření; počet instrukcí není
univerzální převodní konstanta mezi různými cestami nebo ISA.

## 4. Obecná architektura a algoritmy

Základ `d7723d96` už obsahuje dílčí binding caches, immutable type-name metadata,
packed arrays, native plány a ověřené ownership přenosy. Níže je společný
cílový kontrakt; jeho popis není důkaz, že každá uvedená vlastnost v baseline
chybí nebo že její přidání samo vysvětluje naměřený náklad. Realizace naváže
na skutečné implementované části a nahradí obecnou režii podle jejich ceny.

### 4.1 Deklarace, name resolution a veškerá reflexe

DeclarationId odkazuje na sdílený immutable deskriptor. Signature, member
layout, inheritance, source positions, attributes a typové metadata se
sestavují při nutném link/validation kroku a dále se čtou. Instance stav,
closure captures, statics a request/extension hodnoty jsou samostatné mutable
buňky. Reflection getter statického metadata nevytváří znovu celý deskriptor.

První skutečné rozlišení názvu platí jeho bytes a registry query. Následný
binding používá ID a potřebné scope/version guardy. Epochy se vztahují na
dotčenou sémantickou doménu; každá změna každé hodnoty nezvedá jeden globální
counter. Cache má omezenou velikost. Záporný/unresolved lookup nesmí přeskočit
pozdější deklaraci, autoload či alias; callback vždy obnoví potřebná fakta.
Autoload je pozorovatelná call sekvence, včetně přerušené cesty:
[PHP autoload](https://www.php.net/manual/en/language.oop5.autoload.php).

Nákladová forma je
`link(unique declarations) + first_name_bytes + Q*id_guard + output(K) + effects`.
Po ověření bindingu je metadata access O(1); dynamický string stále musí být
rozlišen. Hash map konstanta je amortizovaný/expected parametr, nikoli důkaz
nejhorší ceny všech vstupů. Výčet K položek nebo text o délce B platí svůj
výstup. Member enumeration zachová pořadí, visibility, inheritance i scope.

Celá Reflection family používá tuto rovinu: functions/methods/classes/objects,
parameters/properties/constants/types, attributes, enums, references,
generators/Fibers a extensions. Fresh API objekty mají požadovanou identitu;
invoke, get/set value, attribute instantiation a další execution operace
zůstávají efektovými primitivy. Mutable hodnoty se čtou ve správném okamžiku.
Rozsah API: [PHP Reflection](https://www.php.net/manual/en/book.reflection.php).

### 4.2 Typy, průniky, coercion a variance

Immutable type expression se reprezentuje DAGem s atomy a AND/OR uzly,
sdílenými TypeId a resolved DeclarationId. Konstrukce conjunction neexpanduje
distributivně celý součin alternativ. Vedle grafu zůstává původní validovaná
deklarace pro Reflection, diagnostics a sémantické pořadí. Alias/inheritance
zjednodušení nesmí změnit declaration validity nebo vyvolat další class load.
Validace syntaxe a runtime matching jsou samostatné kroky:
[PHP type declarations](https://www.php.net/manual/en/language.types.declarations.php).

Scalar acceptance využije tag/mask; resolved nominal acceptance dotazy
`(ClassId, TypeId, relevant scope/version)`. Čistý výsledek se sdílí pouze
pokud všechny závislosti platí. Unresolved atom, callable visibility,
coercion či diagnostic má vlastní ordered semantic cestu. Type check vrací
i správně převedenou hodnotu/exception; není vždy jen bool. Typed references
a property constraints se kontrolují na všech povinných write hranicích.

Pro pure DAG vyhodnocení nad jedním vstupem je účet
`I_type = I_DAG(visited type nodes, visited type edges) + sum(I_nominal(query_i))`.
Cena každého skutečného nominal query zahrnuje key/probe, případný ancestry
průchod i vytvoření či obnovu closure po invalidaci nebo vyřazení z cache.
Bez této práce nelze počet nominal queries použít jako instrukční cenu.
Jediný atom a jeden cache miss nad ancestry řetězcem délky H mohou stát Omega(H),
přestože typový DAG má jeden uzel a žádnou hranu. Jen doložený cache hit nad
pevně velkým klíčem má O(1) lookup. Společná příprava closure se účtuje právě
jednou, u query, který ji provedl; invalidace, eviction a spotřeba paměti mají
vlastní účet a omezení. Variance/subtyping
řeší a memoizuje páry uzlů s příslušným scope; bez dalšího důkazu neprohlašujeme
každý relation problem za lineární nebo jednorázový.

Pure opakovaný výpočet s N požadavky, U skutečnými recomputations, cenou c,
probe/key cenou h, invalidací V a přípravou C stojí `N*h + U*c + V+C`.
Poměr proti `N*c` je `U/N + h/c + (V+C)/(N*c)`. Rychlejší miss výpočet má
vlastní koeficient. Mutable object adresa není důkaz stabilního typu obsahu;
reuse nesmí změnit identitu nebo prodloužit život argumentů/výsledků.

Typová algebra v libovolné PHP aplikaci je běžný PHP program. Použije tyto
stejné property/array/call/ownership/IR operace a prokázané souhrny svých těl.
Jádro neurčuje význam uživatelského Type objektu podle jména jeho třídy.

### 4.3 Zdroje, stuby, tokenizer, parser, include a eval

Společná cesta je `resolve/load -> syntax/compile -> link -> execute`.
Read-only artifacts obsahují source/declaration/type/bytecode data a source
map; execution má fresh request/frame/owner stav. Artifact reuse nikdy samo
neznamená opakované vykonání nebo registraci lze přeskočit. `include_once`
se řídí vlastní PHP sémantikou, include/eval zachovají skutečný scope a efekty:
[include](https://www.php.net/manual/en/function.include.php),
[eval](https://www.php.net/manual/en/function.eval.php).

Klíč artifactu obsahuje skutečný zdroj, relevantní config/version/extension
fingerprint a source context. Hash slouží jako index; matematickou rovnost
zdroje vyžaduje porovnání bytes nebo jiný doložený ekvivalent. Shodné mtime či
název souboru nejsou důkaz. Resolution, wrappers, permissions a nutné I/O/error
události se provedou v předepsaném pořadí i při reuse překladu.

Cena je `sum(I_load(actual load events)) + sum(I_artifact(actual cache events))
          + sum(I_translate(actual translation events))
          + sum(I_link_execute_effect(required events))`.
Cache events zahrnují key/probe, validaci, invalidaci a eviction; translation
events zahrnují i opakování po vyřazení, změně závislostí a neúspěšné pokusy.
Počet unikátních zdrojů není počet provedených překladů. Například LRU cache pro
dva artifacts při cyklickém načítání tří zdrojů překládá při každém načtení:
3 000 loads znamená 3 000 translation events, nikoli tři. Účet podle unikátních
překladů platí pouze pod samostatným důkazem jejich zachování v cache po celý
sledovaný běh v rámci paměťového limitu. Každá skutečná práce patří právě do
jedné položky; cena cache missu se nesčítá s inkluzivní cenou téhož překladu.
Pro samotný přijatý lexer/table parser používáme účet nutně čtených bytes,
tokenů, reductions a output uzlů. Jeho linearita je samostatná vlastnost
algoritmu, nikoli všech PHP parserů. PHP tokenizer output a uživatelský AST
zůstávají PHP hodnoty s odpovídající mutability, errors a materializací.
Stub není zvláštní runtime identita: engine metadata se linkují obecně,
userland stub loader/parser běží přes stejné obecné vykonání jako jiný program.

### 4.4 Hodnoty, arrays, properties a vlastnictví

Typed scalar intermediates žijí v SSA registrech. Packed index load nebo
declared-property slot používá doložené layout/type/scope fakta. Associative
array zachová PHP key conversion, insertion order, append/iteration a COW;
references, ArrayAccess, magic, hooks a lazy stavy mají přesnou efektovou cestu.
Property access proof respektuje [PHP property hooks](https://www.php.net/manual/en/language.oop5.property-hooks.php).

Jednotlivé skutečné ownery se přenášejí nebo uvolňují jednou. Ordinary argument
či receiver přes callback vlastní hodnotu; SSA není povolení borrowed heap
argumentu přes PHP call. Retire zpracuje final ownership, children, GC/weak
události a destructor/resurrection přes společné primitivum s omezenou stack
hloubkou. GC výpočet platí skutečné návštěvy grafu; callback mutation může
vyžadovat další práci a není potichu zahrnuta do O(V+E) jedné snapshot fáze.

### 4.5 Vykonání každého PHP těla

Canonical CFG včetně exceptional/resumption hranic se převede do jediného
effect/ownership SSA IR. Constant propagation, dead pure results, dominance
CSE a inline se řídí důkazy aliasů, verzí a pořadí. Register allocation
vychází z živosti. Stack maps rekonstruují všechny PHP-visible živé hodnoty,
argumenty a logické frames. Native efektová operace může pokračovat po helperu;
nepotřebuje VM dispatch pro každý krok, ale nezahodí žádný efekt.

Tento model platí pro methods, closures, callbacks, named/unpacked arguments,
variadics, recursion, dynamic symbols, generators/Fibers a catch/finally.
Rekonstrukce zahrnuje i pending calls/exception replacement a argument snapshots.
Neprokázaná část má svůj canonical krok a cenu. Checked executor a ARM64/x86-64
lowering používají stejná primitiva a proof kontrakty, nikoli odlišnou sémantiku.

Standardní/externí funkce má rovněž vlastní kontrakt a cenu těla. Neprokázaný
extension/SAPI/file/network/process handler je explicitní efektový leaf;
jeho cenu, lifetime a chybu nelze z účtu vyřadit jako mimo benchmark. Pro funkci
dosud nepřítomnou v RPHP je to požadavek k implementaci, ne hotový handler.

## 5. Nejlevnější obecný plán a online příprava

Pro variantu v: `C_v + N*((1-q)*h_v + q*(s_v+b_v))`, kde h je úplný successful
cost, s prefix/guard/materialization/side-exit a b zbývající canonical práce.
Všechny závislosti, odmítnuté proof pokusy a cache/code budgets jsou zahrnuty.
Code/souhrn se sdílí podle declaration/source identity a validních dependency
epochs; cena přípravy se neplatí opakovaně pro každý call site.

Pro už oceněné legální disjunktní intervaly lze použít DP:
`D[i] = min(base[i]+D[i+1], cost[r]+D[end(r)] pro r začínající v i)`.
Při nezávislých cenách a boundary register stavech stojí výběr O(n+K).
Sdílená příprava nebo omezení času/paměti/kódu vyžadují další stav/Pareto výběr;
tvorba kandidátů a globální optimum tímto tvrzením nejsou vyřešeny.

U stabilního kernelu s baseline cenou b, fast cenou h, `delta=b-h>0` a známou
úplnou přípravou `C>0` není budoucí N známé. Online rent/buy pravidlo před
`ceil(C/delta)`-tou návštěvou zaplatí C; do té doby používá baseline. Pro N pod
prahem zaplatí N*delta, pro N od prahu zaplatí `(threshold-1)*delta+C < 2*C`.
Vůči offline minimu `min(N*delta,C)` je dodatečná cena nejvýše dvojnásobná.
Společné N*h se přičte oběma. Je to podmíněný model pro známé stabilní ceny,
ne důkaz 100× či nulové regrese. Profiling/guard ceny patří do b/h/C; mutation,
side exit, proměnlivé cesty a recompilation mají další explicitní účet.

Dolní mez pro pevné nutné požadavky q lze certifikovat pokrytím lowered jednotek:
`LB=min(c^T*x: Ax>=q,x>=0)`. c jsou platné dolní ceny všech přípustných tříd,
nikoli průměry. Dual `y>=0,A^T*y<=c` dává `q^T*y<=I`. Fused práce se nepočítá
dvakrát. Dolní mez neprokazuje existenci plánu; horní účet potřebuje legální
skutečné cesty se všemi efekty a přípravou. Další algebraická eliminace může
změnit q a nesmí být vyloučena pouhým pojmenováním opcode.

## 6. Pokrytí celé PHP sémantiky před realizací

| Rodina | Nutný kontrakt a cena |
| --- | --- |
| syntax/compile/link | Všechny source forms, names, declaration publication, syntax/link/fatal errors a source positions |
| scalar/string/operators | Conversion/comparison/overflow, byte/string délka, diagnostics, suppression a callback observer hranice |
| variables/references | Locals/globals/statics/dynamic symbols, closures, binding a typed alias writes |
| arrays/iteration | Packed/hash/COW, keys/order/append, unpack, destructuring, foreach by-value/by-ref a iterator effects |
| object model | Construction/clone, slots/dynamic fields, visibility, inheritance/traits/enums, hooks/lazy/magic a constants |
| types/reflection | Validation/matching/coercion/variance, veškeré signature/member/type/attribute/runtime metadata a execution API |
| call protocol | Positional/named/variadic/unpack/by-ref, callbacks/FCC, argument evaluation, scope a frame observers |
| control/errors | Branch/loop/match/goto, return/throw/catch/finally, error handlers, assertions/ticks, fatal/exit/shutdown |
| continuations | Yield/yield-from/generator return, Fiber suspend/resume/throw a owner/exception stav |
| lifetime | Heap accounting, retirement, cycle/weak behavior, destructor/resurrection, resources a request end |
| stdlib/extensions | Každý native handler/constant/class včetně unavailable/unsupported branch, body a callbacks |
| world/SAPI | INI/superglobals, streams/wrappers/file/network/process, locale/encoding, clock/RNG a request protocol |

Pro aktuální baseline jsou všechny současné OpCode varianty explicitně
přiřazeny kontraktovým rodinám v
[inventory](performance-php-core-mathematical-design-data.json).
Syntax či standard library bez samostatného opcode zůstává v tabulce požadavků.
Experimental RPHP generics mají oddělené feature kontrakty; nezaměňují se za PHP.
Inventory úplnost není důkaz correctness ani native coverage. Neznámé proof,
jednotkové ceny a chybějící implementace zůstávají otevřené.

## 7. Co zůstává z PHPStanu a jak se postupuje

[Předchozí číselný packet](performance-phpstan-core-mathematical-design-data.json)
zůstává jeden same-input acceptance případ. Jeho 100× rozpočty nejsou parametry
engine heuristiky. Amdahlova podmínka `(1-f)+f/s+g<=0.01` platí pro konkrétní
přesně definovaný účet; není dosaženým ani univerzálním výsledkem tohoto návrhu.

Před realizací musí mít každé primitivum kontrakt, ownership/effect/resume mapu,
parametrickou cenu a explicitní unknowns. Nejprve společný checked model,
potom obě native lowerings. Každá accepted změna musí prokázat obecnou platnost,
same-source correctness, skutečnou instruction/time cenu, cold/fallback/GC
náklady a corpus/holdout/architecture brány. Hranice unsafe a compatibility
testy se zachovají. Nový implementation goal nezačne dalším restartem z main.

Žádná část tohoto dokumentu netvrdí hotovou podporu celého PHP, dosažené
instruction minimum nebo 100× zrychlení. Jde o univerzální zadání, konkrétní
obecné algoritmy a kompoziční závazky, z nichž teprve musí vzniknout ověřené jádro.
