mod common;
use common::run_php;

#[test]
fn test_print_returns_1() {
    assert_eq!(
        run_php(
            r#"<?php
$x = print "hello";
echo $x;
"#
        ),
        "hello1"
    );
}

#[test]
fn test_standalone_print_statement() {
    assert_eq!(run_php("<?php print 'hello';"), "hello");
}

#[test]
fn test_spaceship() {
    assert_eq!(
        run_php(
            r#"<?php
echo (1 <=> 2) . " " . (2 <=> 2) . " " . (3 <=> 2);
"#
        ),
        "-1 0 1"
    );
}

#[test]
fn test_spaceship_strings() {
    assert_eq!(
        run_php(
            r#"<?php
echo ("a" <=> "b") . " " . ("b" <=> "b") . " " . ("c" <=> "b");
"#
        ),
        "-1 0 1"
    );
}

#[test]
fn test_power() {
    assert_eq!(
        run_php(
            r#"<?php
echo 2 ** 10;
"#
        ),
        "1024"
    );
}

#[test]
fn test_power_float() {
    assert_eq!(
        run_php(
            r#"<?php
echo 4 ** 0.5;
"#
        ),
        "2"
    );
}

#[test]
fn test_power_assign() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 2;
$x **= 3;
echo $x;
"#
        ),
        "8"
    );
}

#[test]
fn test_bitwise_and() {
    assert_eq!(
        run_php(
            r#"<?php
echo 0b1100 & 0b1010;
"#
        ),
        "8"
    );
}

#[test]
fn test_bitwise_or() {
    assert_eq!(
        run_php(
            r#"<?php
echo 0b1100 | 0b1010;
"#
        ),
        "14"
    );
}

#[test]
fn test_bitwise_xor() {
    assert_eq!(
        run_php(
            r#"<?php
echo 0b1100 ^ 0b1010;
"#
        ),
        "6"
    );
}

#[test]
fn test_bitwise_not() {
    assert_eq!(
        run_php(
            r#"<?php
echo ~0;
"#
        ),
        "-1"
    );
}

#[test]
fn test_shift_left() {
    assert_eq!(
        run_php(
            r#"<?php
echo 1 << 4;
"#
        ),
        "16"
    );
}

#[test]
fn test_shift_right() {
    assert_eq!(
        run_php(
            r#"<?php
echo 16 >> 2;
"#
        ),
        "4"
    );
}

#[test]
fn test_bitwise_compound_assign() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 0xFF;
$x &= 0x0F;
echo $x . " ";
$y = 0x0F;
$y |= 0xF0;
echo $y . " ";
$z = 0xFF;
$z ^= 0x0F;
echo $z;
"#
        ),
        "15 255 240"
    );
}

#[test]
fn test_shift_compound_assign() {
    assert_eq!(
        run_php(
            r#"<?php
$x = 1;
$x <<= 4;
echo $x . " ";
$x >>= 2;
echo $x;
"#
        ),
        "16 4"
    );
}
