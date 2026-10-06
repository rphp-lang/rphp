# Balanced object release tracking

This checkpoint repairs ownership metadata. It does **not** deliver a
performance improvement or close the PHPStan parity goal.

Baseline `c6ac2dfa` retains the accepted `3d7aa6ae` runtime. Declared or explicitly
tracked objects register one live release candidate, but ordinary and deferred
Value publication discarded that registration's lifecycle bit. Final object
destruction could therefore leave the counter permanently positive. Derived raw
object cloning had the opposite problem: it copied the tracking bit without
registering the new lifetime and could remove the original object's live proof.

Publication now preserves the existing tracking and deep-drop bits. Raw and PHP
object clones register their independent lifetime after field cloning completes.
Value aliases still share one object registration. Existing explicit tracking
for dynamic native objects is retained. The canonical callback planner, layout,
ABI, unsafe inventory and PHP ordering remain unchanged.

Three focused assertions fail on the original runtime and pass with this repair.
Default, no-default and all-feature configurations each pass those three cases
and 94 focused release, weak-object, generator and deep-drop integrations: 291
executions, plus two preparation repeats. Formatting, unsafe policy and the
all-target all-feature check pass. Eleven exact PGO CLI cases agree with PHP;
the two previously recorded baseline PHP gaps remain failures.

The proposed additional global gate in direct retirement is rejected and fully
removed: exploratory instructions increased 0.0964%. The metadata-only repair
is evaluated separately. Its two alternating native exploratory pairs increase
instructions 0.0465%; fresh independent-input PGO pairs measure **73.4634 to
73.4883 billion (+0.0339%)** inside the analysis FIFO. Outputs exactly match PHP,
whose reference count is 10.3386 billion. The recorded analysis times are 6.8398
and 6.7412 seconds, but no timing improvement is claimed from this window.

Five independent pairs on four controls retain exact PHP checksums. Their
confirmed instruction medians change by at most +0.01593% and timing medians by
at most +0.724%. The initial larger timing gains do not repeat and are not
claimed. A separate whole-command RSS check records 622,284 versus 622,320 KiB
(+36 KiB); its whole-command counters are kept distinct from phase counters. Executable text grows 396 bytes; the complete
binary grows 2,744 bytes. Fresh PGO training excludes PHPStan and the controls.
The largest aggregate build peak is below the verified 6 GiB boundary, with no
OOM or swap. Native evidence is x86-64 only; this changes no JIT lowering.

The first baseline regression-proof copy lacked four compile-time inputs and
failed compilation. That infrastructure failure is retained separately; only
the subsequent three actual assertion failures establish the regression proof.

Exact identities, every valid sample, build policy, test commands and resource
boundaries are in [the evidence packet](performance-phpstan-release-tracking-samples.json).
The next performance selection returns to ordinary opcode work, including
classification of slow property reads. The instruction and one-second goals
remain open.
