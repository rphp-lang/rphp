# Physical array copies: architecture hypothesis control

The [architecture comparison](performance-phpstan-architecture.md) needs to
distinguish refcount copies of `Value` from physical copies of `PhpArray`.
The latter clones the element storage and, for indexed arrays, both indexes.
Some standard-library snapshots use that operation. Finding those calls in
source does not establish their frequency or a large native instruction cost.

This diagnostic runs on the frozen owned-frame repair. Production runtime is
unchanged and PHPStan instruction/time parity remains unachieved. The
[packet](performance-phpstan-array-copy-census-data.json) records exact identities,
counts and resource outcomes; the [diagnostic patch](performance-phpstan-array-copy-census-candidate.patch)
applies to that source, rather than silently changing the production baseline.

## Method and results

The `vm-stats` feature counts every physical `PhpArray::clone`, including trait
calls. A temporary thread-local context distinguishes ordinary shared COW,
immutable-literal COW and explicit direct calls. Context guards restore on
unwind and retain no PHP values, references or raw frame pointers. Direct
calls carry Rust caller locations; trait calls without context remain unknown.
The diagnostic limits distinct rows and reports overflow separately.

Three unit controls verify physical versus refcount copies, COW versus unique
mutable access, nested context restoration and disabled accounting. Sixteen
lifetime controls preserve the hash-verified retained PHP/repair output. One
fresh-cache PHPStan request preserves the same five files and twenty findings.
All row counts, copied items and index capacities reconcile to their totals;
there is no site overflow. Shared mutable accesses equal COW copy events.

| Physical-copy cause | Copies | Copied items |
| --- | ---: | ---: |
| Shared array COW | 130,320 | 2,652,018 |
| Immutable literal COW | 48,355 | 4,345 |
| Explicit direct copy | 22,831 | 17,897 |
| Trait call without known context | 20 | 27 |
| Total | 201,526 | 2,674,287 |

The largest copied array has 12,375 items. Seven copies exceed 4,096 items;
they copy 86,403 items in total. The diagnostic observes 7,538,376 unique mutable
array API accesses and 178,675 shared accesses. These are `as_array_mut` entries,
not a count of committed PHP writes. Index-capacity sums are 2,421,206 string
cells and 68,578 integer cells; capacity is not live-key count or an exact byte
traffic measurement.

These counters cover the **whole request after statistics reset**, including
startup. They cannot be divided into the analysis-only native instruction total
or treated as a speedup budget. The retained native repair profile separately
attributes about 0.118 billion self periods to outlined `PhpArray::clone` and
0.058 billion to outlined `as_array_mut`. These omit generic descendants and
inlined work, so they are not the total copy cost.

## Decision

The measured request does not show massive repeated standard-library array
snapshots: direct and unknown calls copy only 17,924 items, whereas ordinary
COW accounts for almost all copied items. This rejects choosing a broad
collection-copy rewrite as the next explanation for the roughly 58-billion
analysis instruction difference. It does not prove optimal COW, bound every
copy's native cost, or settle scaling with the number of analysed files.

Keep collection layout as a separately attributable hypothesis. Resume the
shared operation/ownership review; do not remove required COW or borrow PHP
values across callbacks merely because most copies are small.

## Stronger original-argument counterexample

The earlier frame-entry control checked only `count(func_get_args())`. A stronger
control reads the retained object's identity after overwriting the variadic CV:

```php
class ArgumentOwner {
    function __construct(public int $id) {}
    function __destruct() { echo "drop:", $this->id, "\n"; }
}
function inspect_original_arguments(...$args) {
    $args = [];
    $original = func_get_args();
    echo count($original), ":", get_debug_type($original[0]), ":",
        $original[0]->id ?? "missing", "\n";
}
inspect_original_arguments(new ArgumentOwner(1));
(new ReflectionFunction('inspect_original_arguments'))
    ->invokeArgs([new ArgumentOwner(2)]);
```

PHP retains `ArgumentOwner:1` and `ArgumentOwner:2` and runs both destructors.
The repaired RPHP retains the count but returns `null:missing` in both calls;
it also omits the reflected destructor. Both processes exit zero without stderr.
The retained full control includes direct/reflection/done markers.

Thus matching argument count was insufficient evidence of retention. The
[entry contract](performance-phpstan-temp-ownership-contract.md) must preserve
the actual original values independently of a writable variadic parameter and
retire their final owners through the VM callback protocol. This is a semantic
prerequisite for the ownership redesign, not a newly quantified performance
cause or a claim that the frozen repair is globally correct.
