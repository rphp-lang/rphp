/// E2E tests: frame cleanup — bitmap-driven slot cleanup correctness.
/// Covers: scalar/heap overwrite sequences, mixed frames, generators,
/// and large-frame fallback paths.

mod common;
use common::run_php;

// === Scalar-only frames: cleanup should be a no-op ===

#[test]
fn test_frame_cleanup_scalar_only() {
    // Pure scalar function — no heap slots, bitmap stays 0.
    assert_eq!(run_php("<?php
function add($a, $b) { return $a + $b; }
echo add(1, 2);
echo add(3, 4);
"), "37");
}

// === Mixed scalar/heap frames: bitmap tracks per-slot ===

#[test]
fn test_frame_cleanup_mixed_scalar_heap() {
    // Function that writes both scalar and heap values to different slots.
    // $a is int (scalar), $b is string (heap), $c is int (scalar).
    // Only $b's slot should be in heap_bitmap.
    assert_eq!(run_php("<?php
function mix($a, $b) {
    $c = $a + 1;
    $d = $b . '!';
    return $c . $d;
}
echo mix(10, 'hello');
"), "11hello!");
}

#[test]
fn test_frame_cleanup_heap_overwrite_with_scalar() {
    // Slot starts as heap (string), gets overwritten with scalar (int).
    // Bitmap should clear the heap bit; cleanup should not try to drop scalar.
    assert_eq!(run_php("<?php
function f() {
    $x = 'hello';  // heap
    $x = 42;       // scalar overwrites heap — bitmap clears
    return $x;
}
echo f();
"), "42");
}

#[test]
fn test_frame_cleanup_scalar_overwrite_with_heap() {
    // Slot starts as scalar (int), gets overwritten with heap (string).
    // Bitmap should set the heap bit; cleanup drops the string.
    assert_eq!(run_php("<?php
function f() {
    $x = 42;       // scalar
    $x = 'hello';  // heap overwrites scalar — bitmap sets
    return $x;
}
echo f();
"), "hello");
}

#[test]
fn test_frame_cleanup_multiple_overwrites() {
    // Slot alternates scalar/heap multiple times.
    assert_eq!(run_php("<?php
function f() {
    $x = 1;         // scalar
    $x = 'a';       // heap
    $x = 2;         // scalar
    $x = 'b';       // heap
    $x = 3;         // scalar
    return $x;
}
echo f();
"), "3");
}

// === Heap args from caller (SendVal) ===

#[test]
fn test_frame_cleanup_heap_args_from_caller() {
    // Caller passes strings (heap) as args. Callee is scalar-only body.
    // cleanup must still drop the arg slots correctly.
    assert_eq!(run_php("<?php
function len($s) { return strlen($s); }
echo len('hello');
echo len('world!');
"), "56");
}

// === Return value propagation ===

#[test]
fn test_frame_cleanup_heap_return_to_caller() {
    // Callee returns a string (heap). mark_caller_heap_return should mark
    // the caller's result TMP slot as heap.
    assert_eq!(run_php("<?php
function make_str() { return 'hello'; }
$a = make_str();
$b = make_str();
echo $a . $b;
"), "hellohello");
}

#[test]
fn test_frame_cleanup_scalar_return_to_caller() {
    // Callee returns int (scalar). No bitmap update on caller.
    assert_eq!(run_php("<?php
function make_int() { return 42; }
$a = make_int();
$b = make_int();
echo $a + $b;
"), "84");
}

// === Deep recursion with mixed types ===

#[test]
fn test_frame_cleanup_recursive_mixed() {
    // Recursive function that alternates returning string/int.
    // Each frame has different bitmap state.
    assert_eq!(run_php("<?php
function rec($n) {
    if ($n <= 0) return 'done';
    if ($n % 2 == 0) {
        $x = 'str' . $n;  // heap
        return rec($n - 1);
    } else {
        $x = $n * 10;     // scalar
        return rec($n - 1);
    }
}
echo rec(10);
"), "done");
}

// === Generator resume with heap values ===

#[test]
fn test_frame_cleanup_generator_heap_resume() {
    // Generator yields heap values. frame_restore_slot must update bitmap.
    assert_eq!(run_php("<?php
function gen() {
    $s = 'hello';
    yield $s;
    $s = $s . ' world';
    yield $s;
}
$g = gen();
$g->rewind();
echo $g->current() . '|';
$g->next();
echo $g->current();
"), "hello|hello world");
}

#[test]
fn test_frame_cleanup_generator_scalar_resume() {
    // Generator with scalar values only.
    assert_eq!(run_php("<?php
function gen() {
    $x = 1;
    yield $x;
    $x = $x + 10;
    yield $x;
}
$g = gen();
$g->rewind();
echo $g->current() . '|';
$g->next();
echo $g->current();
"), "1|11");
}

// === Exception/throw cleanup ===

#[test]
fn test_frame_cleanup_exception_with_heap() {
    // Function has heap slots when exception is thrown.
    // cleanup_frame_slots must correctly drop heap slots during unwind.
    assert_eq!(run_php("<?php
function f() {
    $s = 'temporary';
    $arr = [1, 2, 3];
    throw new Exception('boom');
}
try { f(); } catch (Exception $e) { echo $e->getMessage(); }
"), "boom");
}

#[test]
fn test_frame_cleanup_exception_after_heap_overwrite() {
    // Heap slot overwritten with scalar before throw.
    // Must not double-free the old heap value.
    assert_eq!(run_php("<?php
function f() {
    $s = 'hello';
    $s = 42;
    throw new Exception('ok');
}
try { f(); } catch (Exception $e) { echo $e->getMessage(); }
"), "ok");
}

// === Nested calls with bitmap tracking ===

#[test]
fn test_frame_cleanup_nested_mixed_frames() {
    // outer() has heap, inner() is scalar-only.
    // Both frames have correct independent bitmap state.
    assert_eq!(run_php("<?php
function inner($n) { return $n * 2; }
function outer() {
    $s = 'prefix';
    $n = inner(21);
    return $s . $n;
}
echo outer();
"), "prefix42");
}

// === Closure captures (heap values in frame) ===

#[test]
fn test_frame_cleanup_closure_capture_heap() {
    assert_eq!(run_php("<?php
function f() {
    $s = 'captured';
    $fn = function() use ($s) { return $s . '!'; };
    return $fn();
}
echo f();
"), "captured!");
}

// === Many TMP slots ===

#[test]
fn test_frame_cleanup_many_tmps() {
    // Expression with many intermediate TMPs, all scalar.
    assert_eq!(run_php("<?php
function f($a) {
    return $a + 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10;
}
echo f(0);
"), "55");
}

#[test]
fn test_frame_cleanup_many_heap_cvs() {
    // Many CV slots holding strings (heap).
    assert_eq!(run_php("<?php
function f() {
    $a = 'a'; $b = 'b'; $c = 'c'; $d = 'd';
    $e = 'e'; $f = 'f'; $g = 'g'; $h = 'h';
    return $a . $b . $c . $d . $e . $f . $g . $h;
}
echo f();
"), "abcdefgh");
}

// === >64 slot fallback path ===

#[test]
fn test_frame_cleanup_large_frame_fallback_scalar() {
    // Function with >64 local variables → falls back to scan-all cleanup.
    // All scalar — cleanup should be a no-op scan.
    assert_eq!(run_php("<?php
function big() {
    $v00=0;  $v01=1;  $v02=2;  $v03=3;  $v04=4;  $v05=5;  $v06=6;  $v07=7;
    $v08=8;  $v09=9;  $v10=10; $v11=11; $v12=12; $v13=13; $v14=14; $v15=15;
    $v16=16; $v17=17; $v18=18; $v19=19; $v20=20; $v21=21; $v22=22; $v23=23;
    $v24=24; $v25=25; $v26=26; $v27=27; $v28=28; $v29=29; $v30=30; $v31=31;
    $v32=32; $v33=33; $v34=34; $v35=35; $v36=36; $v37=37; $v38=38; $v39=39;
    $v40=40; $v41=41; $v42=42; $v43=43; $v44=44; $v45=45; $v46=46; $v47=47;
    $v48=48; $v49=49; $v50=50; $v51=51; $v52=52; $v53=53; $v54=54; $v55=55;
    $v56=56; $v57=57; $v58=58; $v59=59; $v60=60; $v61=61; $v62=62; $v63=63;
    $v64=64; $v65=65;
    return $v00 + $v65;
}
echo big();
"), "65");
}

#[test]
fn test_frame_cleanup_large_frame_fallback_heap() {
    // Function with >64 locals including heap values → scan-all must drop them.
    assert_eq!(run_php("<?php
function big_heap() {
    $v00='a'; $v01='b'; $v02='c'; $v03='d'; $v04='e'; $v05='f'; $v06='g'; $v07='h';
    $v08='i'; $v09='j'; $v10='k'; $v11='l'; $v12='m'; $v13='n'; $v14='o'; $v15='p';
    $v16='q'; $v17='r'; $v18='s'; $v19='t'; $v20='u'; $v21='v'; $v22='w'; $v23='x';
    $v24='y'; $v25='z'; $v26=0;  $v27=1;  $v28=2;  $v29=3;  $v30=4;  $v31=5;
    $v32=6;  $v33=7;  $v34=8;  $v35=9;  $v36=10; $v37=11; $v38=12; $v39=13;
    $v40=14; $v41=15; $v42=16; $v43=17; $v44=18; $v45=19; $v46=20; $v47=21;
    $v48=22; $v49=23; $v50=24; $v51=25; $v52=26; $v53=27; $v54=28; $v55=29;
    $v56=30; $v57=31; $v58=32; $v59=33; $v60=34; $v61=35; $v62=36; $v63=37;
    $v64='AA'; $v65='BB';
    return $v00 . $v25 . $v64 . $v65;
}
echo big_heap();
"), "azAABB");
}

#[test]
fn test_frame_cleanup_large_frame_fallback_exception() {
    // >64 locals + exception → scan-all cleanup during unwind.
    assert_eq!(run_php("<?php
function big_throw() {
    $v00='s0'; $v01='s1'; $v02='s2'; $v03='s3'; $v04='s4'; $v05='s5'; $v06='s6'; $v07='s7';
    $v08=0;  $v09=1;  $v10=2;  $v11=3;  $v12=4;  $v13=5;  $v14=6;  $v15=7;
    $v16=8;  $v17=9;  $v18=10; $v19=11; $v20=12; $v21=13; $v22=14; $v23=15;
    $v24=16; $v25=17; $v26=18; $v27=19; $v28=20; $v29=21; $v30=22; $v31=23;
    $v32=24; $v33=25; $v34=26; $v35=27; $v36=28; $v37=29; $v38=30; $v39=31;
    $v40=32; $v41=33; $v42=34; $v43=35; $v44=36; $v45=37; $v46=38; $v47=39;
    $v48=40; $v49=41; $v50=42; $v51=43; $v52=44; $v53=45; $v54=46; $v55=47;
    $v56=48; $v57=49; $v58=50; $v59=51; $v60=52; $v61=53; $v62=54; $v63=55;
    $v64='end1'; $v65='end2';
    throw new Exception($v00 . $v64);
}
try { big_throw(); } catch (Exception $e) { echo $e->getMessage(); }
"), "s0end1");
}
