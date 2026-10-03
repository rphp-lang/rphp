# Temporary ownership contract: entry storage comes first

The [architecture comparison](performance-phpstan-architecture.md) now has a
concrete counterexample to a proposed simplifying invariant. A compiler TMP
address is not generally ownerless at function entry in the current repaired
RPHP. Raw argument owners can still occupy it. This rejects a fresh-result
writer selected solely from the temporary's definition and ordinary CFG.
It does not explain the roughly 58-billion native instruction gap.

This checkpoint adds an offline reader and records private diagnostics. No
runtime change is adopted. The [data packet](performance-phpstan-temp-ownership-contract-data.json)
keeps source/binary identities, failures, controls and resource outcomes.
PHPStan instruction/time parity remains open.

## Local lifetime screen

`scripts/analyze-temp-lifetimes.py` reuses the retained bytecode-site census. It
starts each recorded block with unknown ownership, tracks normal successful
scalar publication, source-consuming assignment and explicit release, and drops
facts at unmodelled effects. It rejects modifying/reference uses and multiply
defined scratch slots conservatively. It does not infer fresh-frame ownership
or cross-block facts. The packet lacks complete exceptional and resumable entry
metadata; these results authorize no release elision.

| Whole-request frequency | Hits |
| --- | ---: |
| ReleaseTemps | 38,177,072 |
| Locally empty normal-path ranges | 4,528,552 |
| Static sites contributing those ranges | 279 |
| Locally proven vacant ordinary read result births | 0 |

An assignment can already consume its eligible source. In the captured chains,
no following release interval contains only that one source: earlier operands
and other scratch slots also participate. Treating every `AssignCv` as proof
that the entire next release is empty would be wrong.

The seven reader controls cover combined read/consume events, remaining owners,
address escape, unknown effects and block entry, scalar versus effect markers,
malformed bounds and the absence of a fresh-entry assumption. The retained
screen took about 1.17 seconds before the seventh control was added; it is an
offline planning tool, not a native benchmark.

## Actual CFG diagnostic and its rejected premise

A private Rust solver uses actual instruction geometry, catch roots, single
definitions and conservative joins. It declines generators, finally and
unmodelled nonlocal region entries. Its six focused tests pass. Feature-gated
instrumentation then checks each claimed ownerless result against the live
frame. Clear prefix bits never cause reads of uninitialized scratch bytes;
wide slots use the initialized-slot contract. No alternate writer is enabled.

The first preflight failed before analysis because its wrapper omitted the
phase-control FIFO. That failed observation is preserved, not counted as a
PHPStan run. Correcting the wrapper produced the same five files and twenty
findings as the retained PHP/repair controls, but **nine owner-absence claims
failed**. All nine occur at the first instruction of a variadic constructor,
where a would-be boolean-result scratch slot still holds an Object or String
owner. The sixteen existing lifetime controls preserve output, but most do not
exercise claimed vacant result births; they cannot establish this premise.

Two small programs reproduce the issue independently of PHPStan:

- A variadic constructor called normally consumes its raw positional tail.
  Calling the same constructor through reflection uses detached entry, which
  copies values into the variadic bucket while retaining raw cells.
- An ordinary function called with surplus arguments copies those arguments
  into its introspection snapshot. It clears overlapping declared CV cells,
  but leaves supplied cells beyond declared CVs as owners. Those addresses
  overlap the compiler's first TMPs.

Both controls return `2:2:3` identically in PHP and repaired RPHP. Their entry
violations are at TMP slot 2 of the reflected constructor and TMP slot 1 of the
surplus-argument function. Correct output does not prove that an old owner may
be skipped by a new writer. The global fresh-entry assumption is rejected.

## PHP's geometry and the additional lifetime requirement

The pinned PHP source places extra arguments **after compiled CVs and TMPs**.
Its [frame layout and relocation](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_execute.c#L4325)
move supplied cells there and clear their old locations. The
[variadic receiver](https://github.com/php/php-src/blob/678778973bb4d4185c06ef1320de50f1a975d3c8/Zend/zend_vm_def.h#L5773)
copies those retained arguments into the parameter array. PHP can therefore
retain original argument owners while keeping the compiler's scratch area
separate. It does not simply eliminate all original variadic owners.

A private RPHP entry-normalization attempt consumes detached variadic/capture
cells and clears the complete surplus prefix after snapshotting. It removes
both small entry violations, but fails an additional differential control:

```php
class ArgumentOwner {
    function __destruct() { echo "drop\n"; }
}
function variadic_lifetime(...$args) {
    $args = [];
    echo "inside:", count(func_get_args()), "\n";
}
variadic_lifetime(new ArgumentOwner());
(new ReflectionFunction('variadic_lifetime'))
    ->invokeArgs([new ArgumentOwner()]);
```

PHP reports one original argument inside both calls and invokes the destructor
after both calls. Both the repaired baseline and the entry-normalization
candidate report the original argument count correctly, but omit the destructor
after the reflected call. The candidate does not introduce a new output
regression in this control; the baseline already fails. That still prevents
acceptance of a complete frame-entry/lifetime contract. No native selector,
PGO cycle or full feature matrix follows this failed semantic gate.

## Consequence for the redesign

The next contract must describe three separate storage roles: compiled CV
owners, compiler expression scratch and retained original extra arguments.
Every ordinary, detached, closure and generator entry must establish those
roles before bytecode executes. Captures and bound scope values also need an
explicit reserved location, rather than an assumed vacant physical TMP.

Argument retention must preserve `func_get_arg(s)`, traces, references and COW.
The final retained owner needs the same VM-aware destructor/resource/exception
retirement as other frame owners, before its frame context is lost. Removing a
duplicate raw owner is valid only after that retained owner and its retirement
are established. Clearing cells alone is not the architecture.

Only after this entry contract is valid may compiler dataflow prove fresh
publication and consuming operations share its metadata. Existing exception,
interrupt and GC effects remain required even for an empty cleanup range.
This checkpoint establishes a prerequisite and counterexamples, not a speedup
claim or a complete explanation of the application gap.
