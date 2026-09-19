//! PHP `tokenizer` extension: `token_get_all()`, `token_name()`, `PhpToken`
//! and the `T_*` constants.
//!
//! The scanner is an original state machine over PHP source bytes. It
//! reproduces PHP 8.5's public token contract (identifiers, exact spelling,
//! line numbers and byte offsets) and stays independent of the runtime front
//! end, which consumes a smaller token model.

use crate::compiler::compile::{ClassDef, PropertyDefinition};
use crate::compiler::{make_internal_function, make_internal_method};
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{ObjectLayout, PhpArray, Value, ValueType, make_error_value};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{FunctionCommon, InternalFunction, ParamTypeHint};

pub(crate) const TOKEN_PARSE: i64 = 1;

pub(crate) const T_LNUMBER: i64 = 260;
pub(crate) const T_DNUMBER: i64 = 261;
pub(crate) const T_STRING: i64 = 262;
pub(crate) const T_NAME_FULLY_QUALIFIED: i64 = 263;
pub(crate) const T_NAME_RELATIVE: i64 = 264;
pub(crate) const T_NAME_QUALIFIED: i64 = 265;
pub(crate) const T_VARIABLE: i64 = 266;
pub(crate) const T_INLINE_HTML: i64 = 267;
pub(crate) const T_ENCAPSED_AND_WHITESPACE: i64 = 268;
pub(crate) const T_CONSTANT_ENCAPSED_STRING: i64 = 269;
pub(crate) const T_STRING_VARNAME: i64 = 270;
pub(crate) const T_NUM_STRING: i64 = 271;
pub(crate) const T_INCLUDE: i64 = 272;
pub(crate) const T_INCLUDE_ONCE: i64 = 273;
pub(crate) const T_EVAL: i64 = 274;
pub(crate) const T_REQUIRE: i64 = 275;
pub(crate) const T_REQUIRE_ONCE: i64 = 276;
pub(crate) const T_LOGICAL_OR: i64 = 277;
pub(crate) const T_LOGICAL_XOR: i64 = 278;
pub(crate) const T_LOGICAL_AND: i64 = 279;
pub(crate) const T_PRINT: i64 = 280;
pub(crate) const T_YIELD: i64 = 281;
pub(crate) const T_YIELD_FROM: i64 = 282;
pub(crate) const T_INSTANCEOF: i64 = 283;
pub(crate) const T_NEW: i64 = 284;
pub(crate) const T_CLONE: i64 = 285;
pub(crate) const T_EXIT: i64 = 286;
pub(crate) const T_IF: i64 = 287;
pub(crate) const T_ELSEIF: i64 = 288;
pub(crate) const T_ELSE: i64 = 289;
pub(crate) const T_ENDIF: i64 = 290;
pub(crate) const T_ECHO: i64 = 291;
pub(crate) const T_DO: i64 = 292;
pub(crate) const T_WHILE: i64 = 293;
pub(crate) const T_ENDWHILE: i64 = 294;
pub(crate) const T_FOR: i64 = 295;
pub(crate) const T_ENDFOR: i64 = 296;
pub(crate) const T_FOREACH: i64 = 297;
pub(crate) const T_ENDFOREACH: i64 = 298;
pub(crate) const T_DECLARE: i64 = 299;
pub(crate) const T_ENDDECLARE: i64 = 300;
pub(crate) const T_AS: i64 = 301;
pub(crate) const T_SWITCH: i64 = 302;
pub(crate) const T_ENDSWITCH: i64 = 303;
pub(crate) const T_CASE: i64 = 304;
pub(crate) const T_DEFAULT: i64 = 305;
pub(crate) const T_MATCH: i64 = 306;
pub(crate) const T_BREAK: i64 = 307;
pub(crate) const T_CONTINUE: i64 = 308;
pub(crate) const T_GOTO: i64 = 309;
pub(crate) const T_FUNCTION: i64 = 310;
pub(crate) const T_FN: i64 = 311;
pub(crate) const T_CONST: i64 = 312;
pub(crate) const T_RETURN: i64 = 313;
pub(crate) const T_TRY: i64 = 314;
pub(crate) const T_CATCH: i64 = 315;
pub(crate) const T_FINALLY: i64 = 316;
pub(crate) const T_THROW: i64 = 317;
pub(crate) const T_USE: i64 = 318;
pub(crate) const T_INSTEADOF: i64 = 319;
pub(crate) const T_GLOBAL: i64 = 320;
pub(crate) const T_STATIC: i64 = 321;
pub(crate) const T_ABSTRACT: i64 = 322;
pub(crate) const T_FINAL: i64 = 323;
pub(crate) const T_PRIVATE: i64 = 324;
pub(crate) const T_PROTECTED: i64 = 325;
pub(crate) const T_PUBLIC: i64 = 326;
pub(crate) const T_PRIVATE_SET: i64 = 327;
pub(crate) const T_PROTECTED_SET: i64 = 328;
pub(crate) const T_PUBLIC_SET: i64 = 329;
pub(crate) const T_READONLY: i64 = 330;
pub(crate) const T_VAR: i64 = 331;
pub(crate) const T_UNSET: i64 = 332;
pub(crate) const T_ISSET: i64 = 333;
pub(crate) const T_EMPTY: i64 = 334;
pub(crate) const T_HALT_COMPILER: i64 = 335;
pub(crate) const T_CLASS: i64 = 336;
pub(crate) const T_TRAIT: i64 = 337;
pub(crate) const T_INTERFACE: i64 = 338;
pub(crate) const T_ENUM: i64 = 339;
pub(crate) const T_EXTENDS: i64 = 340;
pub(crate) const T_IMPLEMENTS: i64 = 341;
pub(crate) const T_NAMESPACE: i64 = 342;
pub(crate) const T_LIST: i64 = 343;
pub(crate) const T_ARRAY: i64 = 344;
pub(crate) const T_CALLABLE: i64 = 345;
pub(crate) const T_LINE: i64 = 346;
pub(crate) const T_FILE: i64 = 347;
pub(crate) const T_DIR: i64 = 348;
pub(crate) const T_CLASS_C: i64 = 349;
pub(crate) const T_TRAIT_C: i64 = 350;
pub(crate) const T_METHOD_C: i64 = 351;
pub(crate) const T_FUNC_C: i64 = 352;
pub(crate) const T_PROPERTY_C: i64 = 353;
pub(crate) const T_NS_C: i64 = 354;
pub(crate) const T_ATTRIBUTE: i64 = 355;
pub(crate) const T_PLUS_EQUAL: i64 = 356;
pub(crate) const T_MINUS_EQUAL: i64 = 357;
pub(crate) const T_MUL_EQUAL: i64 = 358;
pub(crate) const T_DIV_EQUAL: i64 = 359;
pub(crate) const T_CONCAT_EQUAL: i64 = 360;
pub(crate) const T_MOD_EQUAL: i64 = 361;
pub(crate) const T_AND_EQUAL: i64 = 362;
pub(crate) const T_OR_EQUAL: i64 = 363;
pub(crate) const T_XOR_EQUAL: i64 = 364;
pub(crate) const T_SL_EQUAL: i64 = 365;
pub(crate) const T_SR_EQUAL: i64 = 366;
pub(crate) const T_COALESCE_EQUAL: i64 = 367;
pub(crate) const T_BOOLEAN_OR: i64 = 368;
pub(crate) const T_BOOLEAN_AND: i64 = 369;
pub(crate) const T_IS_EQUAL: i64 = 370;
pub(crate) const T_IS_NOT_EQUAL: i64 = 371;
pub(crate) const T_IS_IDENTICAL: i64 = 372;
pub(crate) const T_IS_NOT_IDENTICAL: i64 = 373;
pub(crate) const T_IS_SMALLER_OR_EQUAL: i64 = 374;
pub(crate) const T_IS_GREATER_OR_EQUAL: i64 = 375;
pub(crate) const T_SPACESHIP: i64 = 376;
pub(crate) const T_SL: i64 = 377;
pub(crate) const T_SR: i64 = 378;
pub(crate) const T_INC: i64 = 379;
pub(crate) const T_DEC: i64 = 380;
pub(crate) const T_INT_CAST: i64 = 381;
pub(crate) const T_DOUBLE_CAST: i64 = 382;
pub(crate) const T_STRING_CAST: i64 = 383;
pub(crate) const T_ARRAY_CAST: i64 = 384;
pub(crate) const T_OBJECT_CAST: i64 = 385;
pub(crate) const T_BOOL_CAST: i64 = 386;
pub(crate) const T_UNSET_CAST: i64 = 387;
pub(crate) const T_VOID_CAST: i64 = 388;
pub(crate) const T_OBJECT_OPERATOR: i64 = 389;
pub(crate) const T_NULLSAFE_OBJECT_OPERATOR: i64 = 390;
pub(crate) const T_DOUBLE_ARROW: i64 = 391;
pub(crate) const T_COMMENT: i64 = 392;
pub(crate) const T_DOC_COMMENT: i64 = 393;
pub(crate) const T_OPEN_TAG: i64 = 394;
pub(crate) const T_OPEN_TAG_WITH_ECHO: i64 = 395;
pub(crate) const T_CLOSE_TAG: i64 = 396;
pub(crate) const T_WHITESPACE: i64 = 397;
pub(crate) const T_START_HEREDOC: i64 = 398;
pub(crate) const T_END_HEREDOC: i64 = 399;
pub(crate) const T_DOLLAR_OPEN_CURLY_BRACES: i64 = 400;
pub(crate) const T_CURLY_OPEN: i64 = 401;
pub(crate) const T_DOUBLE_COLON: i64 = 402;
pub(crate) const T_NS_SEPARATOR: i64 = 403;
pub(crate) const T_ELLIPSIS: i64 = 404;
pub(crate) const T_COALESCE: i64 = 405;
pub(crate) const T_POW: i64 = 406;
pub(crate) const T_POW_EQUAL: i64 = 407;
pub(crate) const T_PIPE: i64 = 408;
pub(crate) const T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG: i64 = 409;
pub(crate) const T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG: i64 = 410;
pub(crate) const T_BAD_CHARACTER: i64 = 411;

/// Public tokenizer constants in PHP 8.5 declaration order. Numeric identity
/// is part of the contract: scanners compare `token_get_all()` output and
/// PHP-Parser maps the numbers to its own symbols through `constant()`.
pub(crate) const TOKEN_CONSTANTS: &[(&str, i64)] = &[
    ("T_LNUMBER", T_LNUMBER),
    ("T_DNUMBER", T_DNUMBER),
    ("T_STRING", T_STRING),
    ("T_NAME_FULLY_QUALIFIED", T_NAME_FULLY_QUALIFIED),
    ("T_NAME_RELATIVE", T_NAME_RELATIVE),
    ("T_NAME_QUALIFIED", T_NAME_QUALIFIED),
    ("T_VARIABLE", T_VARIABLE),
    ("T_INLINE_HTML", T_INLINE_HTML),
    ("T_ENCAPSED_AND_WHITESPACE", T_ENCAPSED_AND_WHITESPACE),
    ("T_CONSTANT_ENCAPSED_STRING", T_CONSTANT_ENCAPSED_STRING),
    ("T_STRING_VARNAME", T_STRING_VARNAME),
    ("T_NUM_STRING", T_NUM_STRING),
    ("T_INCLUDE", T_INCLUDE),
    ("T_INCLUDE_ONCE", T_INCLUDE_ONCE),
    ("T_EVAL", T_EVAL),
    ("T_REQUIRE", T_REQUIRE),
    ("T_REQUIRE_ONCE", T_REQUIRE_ONCE),
    ("T_LOGICAL_OR", T_LOGICAL_OR),
    ("T_LOGICAL_XOR", T_LOGICAL_XOR),
    ("T_LOGICAL_AND", T_LOGICAL_AND),
    ("T_PRINT", T_PRINT),
    ("T_YIELD", T_YIELD),
    ("T_YIELD_FROM", T_YIELD_FROM),
    ("T_INSTANCEOF", T_INSTANCEOF),
    ("T_NEW", T_NEW),
    ("T_CLONE", T_CLONE),
    ("T_EXIT", T_EXIT),
    ("T_IF", T_IF),
    ("T_ELSEIF", T_ELSEIF),
    ("T_ELSE", T_ELSE),
    ("T_ENDIF", T_ENDIF),
    ("T_ECHO", T_ECHO),
    ("T_DO", T_DO),
    ("T_WHILE", T_WHILE),
    ("T_ENDWHILE", T_ENDWHILE),
    ("T_FOR", T_FOR),
    ("T_ENDFOR", T_ENDFOR),
    ("T_FOREACH", T_FOREACH),
    ("T_ENDFOREACH", T_ENDFOREACH),
    ("T_DECLARE", T_DECLARE),
    ("T_ENDDECLARE", T_ENDDECLARE),
    ("T_AS", T_AS),
    ("T_SWITCH", T_SWITCH),
    ("T_ENDSWITCH", T_ENDSWITCH),
    ("T_CASE", T_CASE),
    ("T_DEFAULT", T_DEFAULT),
    ("T_MATCH", T_MATCH),
    ("T_BREAK", T_BREAK),
    ("T_CONTINUE", T_CONTINUE),
    ("T_GOTO", T_GOTO),
    ("T_FUNCTION", T_FUNCTION),
    ("T_FN", T_FN),
    ("T_CONST", T_CONST),
    ("T_RETURN", T_RETURN),
    ("T_TRY", T_TRY),
    ("T_CATCH", T_CATCH),
    ("T_FINALLY", T_FINALLY),
    ("T_THROW", T_THROW),
    ("T_USE", T_USE),
    ("T_INSTEADOF", T_INSTEADOF),
    ("T_GLOBAL", T_GLOBAL),
    ("T_STATIC", T_STATIC),
    ("T_ABSTRACT", T_ABSTRACT),
    ("T_FINAL", T_FINAL),
    ("T_PRIVATE", T_PRIVATE),
    ("T_PROTECTED", T_PROTECTED),
    ("T_PUBLIC", T_PUBLIC),
    ("T_PRIVATE_SET", T_PRIVATE_SET),
    ("T_PROTECTED_SET", T_PROTECTED_SET),
    ("T_PUBLIC_SET", T_PUBLIC_SET),
    ("T_READONLY", T_READONLY),
    ("T_VAR", T_VAR),
    ("T_UNSET", T_UNSET),
    ("T_ISSET", T_ISSET),
    ("T_EMPTY", T_EMPTY),
    ("T_HALT_COMPILER", T_HALT_COMPILER),
    ("T_CLASS", T_CLASS),
    ("T_TRAIT", T_TRAIT),
    ("T_INTERFACE", T_INTERFACE),
    ("T_ENUM", T_ENUM),
    ("T_EXTENDS", T_EXTENDS),
    ("T_IMPLEMENTS", T_IMPLEMENTS),
    ("T_NAMESPACE", T_NAMESPACE),
    ("T_LIST", T_LIST),
    ("T_ARRAY", T_ARRAY),
    ("T_CALLABLE", T_CALLABLE),
    ("T_LINE", T_LINE),
    ("T_FILE", T_FILE),
    ("T_DIR", T_DIR),
    ("T_CLASS_C", T_CLASS_C),
    ("T_TRAIT_C", T_TRAIT_C),
    ("T_METHOD_C", T_METHOD_C),
    ("T_FUNC_C", T_FUNC_C),
    ("T_PROPERTY_C", T_PROPERTY_C),
    ("T_NS_C", T_NS_C),
    ("T_ATTRIBUTE", T_ATTRIBUTE),
    ("T_PLUS_EQUAL", T_PLUS_EQUAL),
    ("T_MINUS_EQUAL", T_MINUS_EQUAL),
    ("T_MUL_EQUAL", T_MUL_EQUAL),
    ("T_DIV_EQUAL", T_DIV_EQUAL),
    ("T_CONCAT_EQUAL", T_CONCAT_EQUAL),
    ("T_MOD_EQUAL", T_MOD_EQUAL),
    ("T_AND_EQUAL", T_AND_EQUAL),
    ("T_OR_EQUAL", T_OR_EQUAL),
    ("T_XOR_EQUAL", T_XOR_EQUAL),
    ("T_SL_EQUAL", T_SL_EQUAL),
    ("T_SR_EQUAL", T_SR_EQUAL),
    ("T_COALESCE_EQUAL", T_COALESCE_EQUAL),
    ("T_BOOLEAN_OR", T_BOOLEAN_OR),
    ("T_BOOLEAN_AND", T_BOOLEAN_AND),
    ("T_IS_EQUAL", T_IS_EQUAL),
    ("T_IS_NOT_EQUAL", T_IS_NOT_EQUAL),
    ("T_IS_IDENTICAL", T_IS_IDENTICAL),
    ("T_IS_NOT_IDENTICAL", T_IS_NOT_IDENTICAL),
    ("T_IS_SMALLER_OR_EQUAL", T_IS_SMALLER_OR_EQUAL),
    ("T_IS_GREATER_OR_EQUAL", T_IS_GREATER_OR_EQUAL),
    ("T_SPACESHIP", T_SPACESHIP),
    ("T_SL", T_SL),
    ("T_SR", T_SR),
    ("T_INC", T_INC),
    ("T_DEC", T_DEC),
    ("T_INT_CAST", T_INT_CAST),
    ("T_DOUBLE_CAST", T_DOUBLE_CAST),
    ("T_STRING_CAST", T_STRING_CAST),
    ("T_ARRAY_CAST", T_ARRAY_CAST),
    ("T_OBJECT_CAST", T_OBJECT_CAST),
    ("T_BOOL_CAST", T_BOOL_CAST),
    ("T_UNSET_CAST", T_UNSET_CAST),
    ("T_VOID_CAST", T_VOID_CAST),
    ("T_OBJECT_OPERATOR", T_OBJECT_OPERATOR),
    ("T_NULLSAFE_OBJECT_OPERATOR", T_NULLSAFE_OBJECT_OPERATOR),
    ("T_DOUBLE_ARROW", T_DOUBLE_ARROW),
    ("T_COMMENT", T_COMMENT),
    ("T_DOC_COMMENT", T_DOC_COMMENT),
    ("T_OPEN_TAG", T_OPEN_TAG),
    ("T_OPEN_TAG_WITH_ECHO", T_OPEN_TAG_WITH_ECHO),
    ("T_CLOSE_TAG", T_CLOSE_TAG),
    ("T_WHITESPACE", T_WHITESPACE),
    ("T_START_HEREDOC", T_START_HEREDOC),
    ("T_END_HEREDOC", T_END_HEREDOC),
    ("T_DOLLAR_OPEN_CURLY_BRACES", T_DOLLAR_OPEN_CURLY_BRACES),
    ("T_CURLY_OPEN", T_CURLY_OPEN),
    ("T_DOUBLE_COLON", T_DOUBLE_COLON),
    ("T_PAAMAYIM_NEKUDOTAYIM", T_DOUBLE_COLON),
    ("T_NS_SEPARATOR", T_NS_SEPARATOR),
    ("T_ELLIPSIS", T_ELLIPSIS),
    ("T_COALESCE", T_COALESCE),
    ("T_POW", T_POW),
    ("T_POW_EQUAL", T_POW_EQUAL),
    ("T_PIPE", T_PIPE),
    (
        "T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG",
        T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG,
    ),
    (
        "T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG",
        T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG,
    ),
    ("T_BAD_CHARACTER", T_BAD_CHARACTER),
    ("TOKEN_PARSE", TOKEN_PARSE),
];

/// Value of a public tokenizer constant, by exact (case-sensitive) name.
pub(crate) fn constant_value(name: &str) -> Option<i64> {
    TOKEN_CONSTANTS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, value)| *value)
}

/// Canonical `T_*` name of a named token identifier. `T_DOUBLE_COLON` wins
/// over its `T_PAAMAYIM_NEKUDOTAYIM` alias, as in PHP.
pub(crate) fn token_name(id: i64) -> Option<&'static str> {
    if !(T_LNUMBER..=T_BAD_CHARACTER).contains(&id) {
        return None;
    }
    TOKEN_CONSTANTS
        .iter()
        .find(|(_, value)| *value == id)
        .map(|(name, _)| *name)
}

/// Reserved words and magic constants, matched case-insensitively.
fn keyword_id(word: &[u8]) -> Option<i64> {
    // `__halt_compiler` is the longest keyword; longer words never match.
    if word.len() > 15 {
        return None;
    }
    let mut lowered = [0u8; 15];
    for (slot, byte) in lowered.iter_mut().zip(word) {
        *slot = byte.to_ascii_lowercase();
    }
    let id = match &lowered[..word.len()] {
        b"abstract" => T_ABSTRACT,
        b"and" => T_LOGICAL_AND,
        b"array" => T_ARRAY,
        b"as" => T_AS,
        b"break" => T_BREAK,
        b"callable" => T_CALLABLE,
        b"case" => T_CASE,
        b"catch" => T_CATCH,
        b"class" => T_CLASS,
        b"clone" => T_CLONE,
        b"const" => T_CONST,
        b"continue" => T_CONTINUE,
        b"declare" => T_DECLARE,
        b"default" => T_DEFAULT,
        b"die" => T_EXIT,
        b"do" => T_DO,
        b"echo" => T_ECHO,
        b"else" => T_ELSE,
        b"elseif" => T_ELSEIF,
        b"empty" => T_EMPTY,
        b"enddeclare" => T_ENDDECLARE,
        b"endfor" => T_ENDFOR,
        b"endforeach" => T_ENDFOREACH,
        b"endif" => T_ENDIF,
        b"endswitch" => T_ENDSWITCH,
        b"endwhile" => T_ENDWHILE,
        b"enum" => T_ENUM,
        b"eval" => T_EVAL,
        b"exit" => T_EXIT,
        b"extends" => T_EXTENDS,
        b"final" => T_FINAL,
        b"finally" => T_FINALLY,
        b"fn" => T_FN,
        b"for" => T_FOR,
        b"foreach" => T_FOREACH,
        b"function" => T_FUNCTION,
        b"global" => T_GLOBAL,
        b"goto" => T_GOTO,
        b"if" => T_IF,
        b"implements" => T_IMPLEMENTS,
        b"include" => T_INCLUDE,
        b"include_once" => T_INCLUDE_ONCE,
        b"instanceof" => T_INSTANCEOF,
        b"insteadof" => T_INSTEADOF,
        b"interface" => T_INTERFACE,
        b"isset" => T_ISSET,
        b"list" => T_LIST,
        b"match" => T_MATCH,
        b"namespace" => T_NAMESPACE,
        b"new" => T_NEW,
        b"or" => T_LOGICAL_OR,
        b"print" => T_PRINT,
        b"private" => T_PRIVATE,
        b"protected" => T_PROTECTED,
        b"public" => T_PUBLIC,
        b"readonly" => T_READONLY,
        b"require" => T_REQUIRE,
        b"require_once" => T_REQUIRE_ONCE,
        b"return" => T_RETURN,
        b"static" => T_STATIC,
        b"switch" => T_SWITCH,
        b"throw" => T_THROW,
        b"trait" => T_TRAIT,
        b"try" => T_TRY,
        b"unset" => T_UNSET,
        b"use" => T_USE,
        b"var" => T_VAR,
        b"while" => T_WHILE,
        b"xor" => T_LOGICAL_XOR,
        b"yield" => T_YIELD,
        b"__halt_compiler" => T_HALT_COMPILER,
        b"__class__" => T_CLASS_C,
        b"__dir__" => T_DIR,
        b"__file__" => T_FILE,
        b"__function__" => T_FUNC_C,
        b"__line__" => T_LINE,
        b"__method__" => T_METHOD_C,
        b"__namespace__" => T_NS_C,
        b"__property__" => T_PROPERTY_C,
        b"__trait__" => T_TRAIT_C,
        _ => return None,
    };
    Some(id)
}

fn cast_id(word: &[u8]) -> Option<i64> {
    if word.len() > 7 {
        return None;
    }
    let mut lowered = [0u8; 7];
    for (slot, byte) in lowered.iter_mut().zip(word) {
        *slot = byte.to_ascii_lowercase();
    }
    let id = match &lowered[..word.len()] {
        b"int" | b"integer" => T_INT_CAST,
        b"bool" | b"boolean" => T_BOOL_CAST,
        b"float" | b"double" | b"real" => T_DOUBLE_CAST,
        b"string" | b"binary" => T_STRING_CAST,
        b"array" => T_ARRAY_CAST,
        b"object" => T_OBJECT_CAST,
        b"unset" => T_UNSET_CAST,
        b"void" => T_VOID_CAST,
        _ => return None,
    };
    Some(id)
}

#[inline]
fn is_label_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic() || byte >= 0x80
}

#[inline]
fn is_label_char(byte: u8) -> bool {
    is_label_start(byte) || byte.is_ascii_digit()
}

#[inline]
fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Length of the newline sequence starting at `at`: `\r\n`, `\n` or `\r`.
fn newline_len(source: &[u8], at: usize) -> usize {
    match source.get(at) {
        Some(b'\n') => 1,
        Some(b'\r') if source.get(at + 1) == Some(&b'\n') => 2,
        Some(b'\r') => 1,
        _ => 0,
    }
}

fn count_newlines(bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => count += 1,
            b'\r' => {
                count += 1;
                if bytes.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    count
}

/// One public token: a named identifier at or above `T_LNUMBER`, or the byte
/// value of a single-character token (`b"` keeps the quote's identity).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RawToken {
    pub id: i64,
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

impl RawToken {
    fn is_trivia(&self) -> bool {
        matches!(self.id, T_WHITESPACE | T_COMMENT | T_DOC_COMMENT)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Initial,
    Scripting,
    LookingForProperty,
    DoubleQuotes,
    Backquote,
    Heredoc {
        label_start: usize,
        label_end: usize,
        nowdoc: bool,
    },
    VarOffset,
    LookingForVarname,
}

struct Scanner<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
    states: Vec<State>,
    tokens: Vec<RawToken>,
    /// Non-trivia tokens still admitted after `__halt_compiler` before the
    /// remaining source is published as one inline-HTML token.
    halt_budget: Option<u8>,
    finished: bool,
}

/// Tokenize PHP source bytes into PHP's public token stream.
pub(crate) fn scan(source: &[u8]) -> Vec<RawToken> {
    Scanner {
        src: source,
        pos: 0,
        line: 1,
        states: vec![State::Initial],
        tokens: Vec::new(),
        halt_budget: None,
        finished: false,
    }
    .run()
}

impl<'a> Scanner<'a> {
    fn run(mut self) -> Vec<RawToken> {
        while !self.finished && self.pos < self.src.len() {
            match self.state() {
                State::Initial => self.scan_initial(),
                State::Scripting => self.scan_scripting(),
                State::LookingForProperty => self.scan_property(),
                State::DoubleQuotes => self.scan_encapsed(b'"'),
                State::Backquote => self.scan_encapsed(b'`'),
                State::Heredoc {
                    label_start,
                    label_end,
                    nowdoc,
                } => self.scan_heredoc(label_start, label_end, nowdoc),
                State::VarOffset => self.scan_var_offset(),
                State::LookingForVarname => self.scan_varname(),
            }
        }
        self.tokens
    }

    fn state(&self) -> State {
        self.states.last().copied().unwrap_or(State::Initial)
    }

    fn push(&mut self, state: State) {
        self.states.push(state);
    }

    fn pop(&mut self) {
        if self.states.len() > 1 {
            self.states.pop();
        }
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.src.get(at).copied()
    }

    fn emit(&mut self, id: i64, start: usize, end: usize) {
        self.tokens.push(RawToken {
            id,
            start,
            end,
            line: self.line,
        });
        self.line += count_newlines(&self.src[start..end]);
        self.pos = end;
        if let Some(budget) = self.halt_budget
            && !matches!(id, T_WHITESPACE | T_COMMENT | T_DOC_COMMENT)
        {
            let remaining = budget.saturating_sub(1);
            self.halt_budget = Some(remaining);
            if remaining == 0 {
                if self.pos < self.src.len() {
                    self.tokens.push(RawToken {
                        id: T_INLINE_HTML,
                        start: self.pos,
                        end: self.src.len(),
                        line: self.line,
                    });
                }
                self.pos = self.src.len();
                self.finished = true;
            }
        }
    }

    fn emit_char(&mut self, at: usize) {
        self.emit(i64::from(self.src[at]), at, at + 1);
    }

    fn scan_initial(&mut self) {
        let src = self.src;
        let start = self.pos;
        let mut index = start;
        while index + 1 < src.len() {
            if src[index] == b'<' && src[index + 1] == b'?' {
                if src.get(index + 2) == Some(&b'=') {
                    if index > start {
                        self.emit(T_INLINE_HTML, start, index);
                    }
                    self.emit(T_OPEN_TAG_WITH_ECHO, index, index + 3);
                    self.states = vec![State::Scripting];
                    return;
                }
                if src.len() >= index + 5 && src[index + 2..index + 5].eq_ignore_ascii_case(b"php")
                {
                    let after = index + 5;
                    let tag_end = match src.get(after) {
                        None => Some(after),
                        Some(byte) if is_whitespace(*byte) => {
                            Some(after + newline_len(src, after).max(1))
                        }
                        _ => None,
                    };
                    if let Some(end) = tag_end {
                        if index > start {
                            self.emit(T_INLINE_HTML, start, index);
                        }
                        self.emit(T_OPEN_TAG, index, end);
                        self.states = vec![State::Scripting];
                        return;
                    }
                }
            }
            index += 1;
        }
        self.emit(T_INLINE_HTML, start, src.len());
    }

    fn scan_scripting(&mut self) {
        let src = self.src;
        let start = self.pos;
        let byte = src[start];
        let next = self.byte(start + 1);
        match byte {
            b'?' if next == Some(&b'>').copied() => {
                let end = start + 2 + newline_len(src, start + 2);
                self.emit(T_CLOSE_TAG, start, end);
                self.states = vec![State::Initial];
            }
            b' ' | b'\t' | b'\n' | b'\r' => {
                let mut end = start;
                while end < src.len() && is_whitespace(src[end]) {
                    end += 1;
                }
                self.emit(T_WHITESPACE, start, end);
            }
            b'#' if next == Some(b'[') => self.emit(T_ATTRIBUTE, start, start + 2),
            b'#' => self.scan_line_comment(start),
            b'/' if next == Some(b'/') => self.scan_line_comment(start),
            b'/' if next == Some(b'*') => self.scan_block_comment(start),
            b'(' => {
                if let Some((id, end)) = self.cast_at(start) {
                    self.emit(id, start, end);
                } else {
                    self.emit_char(start);
                }
            }
            b'<' if next == Some(b'<') && self.byte(start + 2) == Some(b'<') => {
                if let Some((end, label_start, label_end, nowdoc)) = self.heredoc_start_at(start) {
                    self.emit(T_START_HEREDOC, start, end);
                    self.push(State::Heredoc {
                        label_start,
                        label_end,
                        nowdoc,
                    });
                } else {
                    self.scan_operator(start);
                }
            }
            b'$' => {
                if next.is_some_and(is_label_start) {
                    let end = self.label_end(start + 1);
                    self.emit(T_VARIABLE, start, end);
                } else {
                    self.emit_char(start);
                }
            }
            b'\\' => {
                if next.is_some_and(is_label_start) {
                    let end = self.qualified_name_end(start);
                    self.emit(T_NAME_FULLY_QUALIFIED, start, end);
                } else {
                    self.emit(T_NS_SEPARATOR, start, start + 1);
                }
            }
            b'\'' | b'"' | b'`' => self.scan_quoted(start, start),
            b'.' if next.is_some_and(|byte| byte.is_ascii_digit()) => self.scan_number(start),
            b'0'..=b'9' => self.scan_number(start),
            b'{' => {
                self.emit_char(start);
                self.push(State::Scripting);
            }
            b'}' => {
                self.emit_char(start);
                self.pop();
            }
            _ if is_label_start(byte) => self.scan_label(start),
            _ => self.scan_operator(start),
        }
    }

    fn scan_line_comment(&mut self, start: usize) {
        let src = self.src;
        let mut end = start + if src[start] == b'#' { 1 } else { 2 };
        while end < src.len() {
            match src[end] {
                b'\n' | b'\r' => break,
                b'?' if src.get(end + 1) == Some(&b'>') => break,
                _ => end += 1,
            }
        }
        self.emit(T_COMMENT, start, end);
    }

    fn scan_block_comment(&mut self, start: usize) {
        let src = self.src;
        let doc = src.get(start + 2) == Some(&b'*')
            && src.get(start + 3).is_some_and(|byte| is_whitespace(*byte));
        let mut index = start + 2;
        let end = loop {
            if index + 1 >= src.len() {
                break src.len();
            }
            if src[index] == b'*' && src[index + 1] == b'/' {
                break index + 2;
            }
            index += 1;
        };
        self.emit(if doc { T_DOC_COMMENT } else { T_COMMENT }, start, end);
    }

    fn cast_at(&self, start: usize) -> Option<(i64, usize)> {
        let src = self.src;
        let mut index = start + 1;
        while index < src.len() && matches!(src[index], b' ' | b'\t') {
            index += 1;
        }
        let word_start = index;
        while index < src.len() && src[index].is_ascii_alphabetic() {
            index += 1;
        }
        let id = cast_id(&src[word_start..index])?;
        while index < src.len() && matches!(src[index], b' ' | b'\t') {
            index += 1;
        }
        (src.get(index) == Some(&b')')).then_some((id, index + 1))
    }

    fn heredoc_start_at(&self, start: usize) -> Option<(usize, usize, usize, bool)> {
        let src = self.src;
        let mut index = start + 3;
        while index < src.len() && matches!(src[index], b' ' | b'\t') {
            index += 1;
        }
        let quote = match src.get(index) {
            Some(b'\'') => {
                index += 1;
                Some(b'\'')
            }
            Some(b'"') => {
                index += 1;
                Some(b'"')
            }
            _ => None,
        };
        if !src.get(index).is_some_and(|byte| is_label_start(*byte)) {
            return None;
        }
        let label_start = index;
        let label_end = self.label_end(index);
        index = label_end;
        if let Some(quote) = quote {
            if src.get(index) != Some(&quote) {
                return None;
            }
            index += 1;
        }
        let newline = newline_len(src, index);
        if newline == 0 {
            return None;
        }
        Some((
            index + newline,
            label_start,
            label_end,
            quote == Some(b'\''),
        ))
    }

    fn label_end(&self, start: usize) -> usize {
        let src = self.src;
        let mut end = start;
        while end < src.len() && is_label_char(src[end]) {
            end += 1;
        }
        end
    }

    /// End of a `\a\b` style name whose next component starts at `start`
    /// (either a backslash followed by a label or a label itself). A trailing
    /// backslash never belongs to the name.
    fn qualified_name_end(&self, start: usize) -> usize {
        let src = self.src;
        let mut end = if src[start] == b'\\' {
            start + 1
        } else {
            start
        };
        end = self.label_end(end);
        while src.get(end) == Some(&b'\\')
            && src.get(end + 1).is_some_and(|byte| is_label_start(*byte))
        {
            end = self.label_end(end + 1);
        }
        end
    }

    fn scan_label(&mut self, start: usize) {
        let src = self.src;
        let end = self.label_end(start);
        if end == start + 1 && matches!(src[start], b'b' | b'B') {
            if matches!(src.get(end), Some(b'\'') | Some(b'"')) {
                self.scan_quoted(start, end);
                return;
            }
            if src[end..].starts_with(b"<<<")
                && let Some((heredoc_end, label_start, label_end, nowdoc)) =
                    self.heredoc_start_at(end)
            {
                self.emit(T_START_HEREDOC, start, heredoc_end);
                self.push(State::Heredoc {
                    label_start,
                    label_end,
                    nowdoc,
                });
                return;
            }
        }
        if src.get(end) == Some(&b'\\')
            && src.get(end + 1).is_some_and(|byte| is_label_start(*byte))
        {
            let name_end = self.qualified_name_end(start);
            let id = if src[start..end].eq_ignore_ascii_case(b"namespace") {
                T_NAME_RELATIVE
            } else {
                T_NAME_QUALIFIED
            };
            self.emit(id, start, name_end);
            return;
        }
        let word = &src[start..end];
        let Some(id) = keyword_id(word) else {
            self.emit(T_STRING, start, end);
            return;
        };
        match id {
            T_YIELD => {
                if let Some(from_end) = self.yield_from_end(end) {
                    self.emit(T_YIELD_FROM, start, from_end);
                    return;
                }
            }
            T_ENUM => {
                if !self.enum_declares_type(end) {
                    self.emit(T_STRING, start, end);
                    return;
                }
            }
            T_PUBLIC | T_PROTECTED | T_PRIVATE => {
                if src.len() >= end + 5
                    && src[end] == b'('
                    && src[end + 1..end + 4].eq_ignore_ascii_case(b"set")
                    && src[end + 4] == b')'
                {
                    let set_id = match id {
                        T_PUBLIC => T_PUBLIC_SET,
                        T_PROTECTED => T_PROTECTED_SET,
                        _ => T_PRIVATE_SET,
                    };
                    self.emit(set_id, start, end + 5);
                    return;
                }
            }
            T_HALT_COMPILER => {
                self.emit(T_HALT_COMPILER, start, end);
                self.halt_budget = Some(3);
                return;
            }
            _ => {}
        }
        self.emit(id, start, end);
    }

    fn yield_from_end(&self, after_yield: usize) -> Option<usize> {
        let src = self.src;
        let mut index = after_yield;
        while index < src.len() && is_whitespace(src[index]) {
            index += 1;
        }
        if index == after_yield || src.len() < index + 4 {
            return None;
        }
        if !src[index..index + 4].eq_ignore_ascii_case(b"from") {
            return None;
        }
        (!src.get(index + 4).is_some_and(|byte| is_label_char(*byte))).then_some(index + 4)
    }

    /// First position at or after `start` that is neither whitespace nor a
    /// comment.
    fn skip_trivia(&self, start: usize) -> usize {
        let src = self.src;
        let mut index = start;
        loop {
            if index < src.len() && is_whitespace(src[index]) {
                index += 1;
            } else if src[index..].starts_with(b"/*") {
                index = src[index + 2..]
                    .windows(2)
                    .position(|pair| pair == b"*/")
                    .map_or(src.len(), |offset| index + 2 + offset + 2);
            } else if src[index..].starts_with(b"//") || src.get(index) == Some(&b'#') {
                while index < src.len() && !matches!(src[index], b'\n' | b'\r') {
                    index += 1;
                }
            } else {
                return index;
            }
        }
    }

    /// `enum` is a keyword only when whitespace or comments and then a name
    /// follow it, and the name is not `extends`/`implements`.
    fn enum_declares_type(&self, after_enum: usize) -> bool {
        let src = self.src;
        let index = self.skip_trivia(after_enum);
        if index == after_enum || !src.get(index).is_some_and(|byte| is_label_start(*byte)) {
            return false;
        }
        let rest = &src[index..];
        !(rest.len() >= 7 && rest[..7].eq_ignore_ascii_case(b"extends")
            || rest.len() >= 10 && rest[..10].eq_ignore_ascii_case(b"implements"))
    }

    fn scan_number(&mut self, start: usize) {
        let src = self.src;
        if src[start] == b'0'
            && let Some(prefix) = src.get(start + 1)
        {
            let radix: Option<(u32, fn(u8) -> bool)> = match prefix {
                b'x' | b'X' => Some((16, |byte| byte.is_ascii_hexdigit())),
                b'b' | b'B' => Some((2, |byte| matches!(byte, b'0' | b'1'))),
                b'o' | b'O' => Some((8, |byte| matches!(byte, b'0'..=b'7'))),
                _ => None,
            };
            if let Some((radix, is_digit)) = radix
                && src.get(start + 2).is_some_and(|byte| is_digit(*byte))
            {
                let end = digits_end(src, start + 2, is_digit);
                let id = if fits_in_long(&src[start + 2..end], radix) {
                    T_LNUMBER
                } else {
                    T_DNUMBER
                };
                self.emit(id, start, end);
                return;
            }
        }
        let is_decimal = |byte: u8| byte.is_ascii_digit();
        let integer_end = digits_end(src, start, is_decimal);
        let mut end = integer_end;
        let mut is_float = false;
        if src.get(end) == Some(&b'.') {
            let fraction_end = digits_end(src, end + 1, is_decimal);
            if fraction_end > end + 1 || integer_end > start {
                end = fraction_end;
                is_float = true;
            }
        }
        if matches!(src.get(end), Some(b'e') | Some(b'E')) {
            let mut exponent = end + 1;
            if matches!(src.get(exponent), Some(b'+') | Some(b'-')) {
                exponent += 1;
            }
            if src.get(exponent).is_some_and(|byte| byte.is_ascii_digit()) {
                end = digits_end(src, exponent, is_decimal);
                is_float = true;
            }
        }
        if is_float {
            self.emit(T_DNUMBER, start, end);
            return;
        }
        let digits = &src[start..end];
        let fits = if digits.len() > 1 && digits[0] == b'0' {
            // Legacy octal. The valid prefix decides overflow; an invalid
            // digit stays in the integer token for the parser to report.
            let valid = digits[1..]
                .iter()
                .position(|byte| matches!(byte, b'8' | b'9'))
                .map_or(&digits[1..], |invalid| &digits[1..1 + invalid]);
            fits_in_long(valid, 8)
        } else {
            fits_in_long(digits, 10)
        };
        self.emit(if fits { T_LNUMBER } else { T_DNUMBER }, start, end);
    }

    fn scan_quoted(&mut self, start: usize, quote_at: usize) {
        let src = self.src;
        match src[quote_at] {
            b'\'' => {
                let mut index = quote_at + 1;
                loop {
                    match src.get(index) {
                        None => {
                            self.emit(T_ENCAPSED_AND_WHITESPACE, start, src.len());
                            return;
                        }
                        Some(b'\\') => index += 2,
                        Some(b'\'') => {
                            self.emit(T_CONSTANT_ENCAPSED_STRING, start, index + 1);
                            return;
                        }
                        Some(_) => index += 1,
                    }
                }
            }
            b'"' => {
                let mut index = quote_at + 1;
                let mut terminated = false;
                let mut interpolated = false;
                while index < src.len() {
                    match src[index] {
                        b'\\' => index += 2,
                        b'"' => {
                            terminated = true;
                            break;
                        }
                        b'$' if src
                            .get(index + 1)
                            .is_some_and(|byte| is_label_start(*byte) || *byte == b'{') =>
                        {
                            interpolated = true;
                            break;
                        }
                        b'{' if src.get(index + 1) == Some(&b'$') => {
                            interpolated = true;
                            break;
                        }
                        _ => index += 1,
                    }
                }
                if terminated && !interpolated {
                    self.emit(T_CONSTANT_ENCAPSED_STRING, start, index + 1);
                    return;
                }
                self.emit(i64::from(b'"'), start, quote_at + 1);
                self.push(State::DoubleQuotes);
            }
            _ => {
                self.emit(i64::from(b'`'), start, quote_at + 1);
                self.push(State::Backquote);
            }
        }
    }

    fn scan_operator(&mut self, start: usize) {
        let src = self.src;
        let next = self.byte(start + 1);
        let third = self.byte(start + 2);
        let (id, len) = match src[start] {
            b'<' => match (next, third) {
                (Some(b'='), Some(b'>')) => (T_SPACESHIP, 3),
                (Some(b'<'), Some(b'=')) => (T_SL_EQUAL, 3),
                (Some(b'='), _) => (T_IS_SMALLER_OR_EQUAL, 2),
                (Some(b'>'), _) => (T_IS_NOT_EQUAL, 2),
                (Some(b'<'), _) => (T_SL, 2),
                _ => (i64::from(b'<'), 1),
            },
            b'>' => match (next, third) {
                (Some(b'>'), Some(b'=')) => (T_SR_EQUAL, 3),
                (Some(b'='), _) => (T_IS_GREATER_OR_EQUAL, 2),
                (Some(b'>'), _) => (T_SR, 2),
                _ => (i64::from(b'>'), 1),
            },
            b'=' => match (next, third) {
                (Some(b'='), Some(b'=')) => (T_IS_IDENTICAL, 3),
                (Some(b'='), _) => (T_IS_EQUAL, 2),
                (Some(b'>'), _) => (T_DOUBLE_ARROW, 2),
                _ => (i64::from(b'='), 1),
            },
            b'!' => match (next, third) {
                (Some(b'='), Some(b'=')) => (T_IS_NOT_IDENTICAL, 3),
                (Some(b'='), _) => (T_IS_NOT_EQUAL, 2),
                _ => (i64::from(b'!'), 1),
            },
            b'*' => match (next, third) {
                (Some(b'*'), Some(b'=')) => (T_POW_EQUAL, 3),
                (Some(b'*'), _) => (T_POW, 2),
                (Some(b'='), _) => (T_MUL_EQUAL, 2),
                _ => (i64::from(b'*'), 1),
            },
            b'.' => match (next, third) {
                (Some(b'.'), Some(b'.')) => (T_ELLIPSIS, 3),
                (Some(b'='), _) => (T_CONCAT_EQUAL, 2),
                _ => (i64::from(b'.'), 1),
            },
            b'?' => match (next, third) {
                (Some(b'?'), Some(b'=')) => (T_COALESCE_EQUAL, 3),
                (Some(b'-'), Some(b'>')) => (T_NULLSAFE_OBJECT_OPERATOR, 3),
                (Some(b'?'), _) => (T_COALESCE, 2),
                _ => (i64::from(b'?'), 1),
            },
            b'-' => match next {
                Some(b'>') => (T_OBJECT_OPERATOR, 2),
                Some(b'-') => (T_DEC, 2),
                Some(b'=') => (T_MINUS_EQUAL, 2),
                _ => (i64::from(b'-'), 1),
            },
            b'+' => match next {
                Some(b'+') => (T_INC, 2),
                Some(b'=') => (T_PLUS_EQUAL, 2),
                _ => (i64::from(b'+'), 1),
            },
            b'/' => match next {
                Some(b'=') => (T_DIV_EQUAL, 2),
                _ => (i64::from(b'/'), 1),
            },
            b'%' => match next {
                Some(b'=') => (T_MOD_EQUAL, 2),
                _ => (i64::from(b'%'), 1),
            },
            b'^' => match next {
                Some(b'=') => (T_XOR_EQUAL, 2),
                _ => (i64::from(b'^'), 1),
            },
            b'|' => match next {
                Some(b'|') => (T_BOOLEAN_OR, 2),
                Some(b'=') => (T_OR_EQUAL, 2),
                Some(b'>') => (T_PIPE, 2),
                _ => (i64::from(b'|'), 1),
            },
            b'&' => match next {
                Some(b'&') => (T_BOOLEAN_AND, 2),
                Some(b'=') => (T_AND_EQUAL, 2),
                _ => {
                    let index = self.skip_trivia(start + 1);
                    let followed =
                        src.get(index) == Some(&b'$') || src[index..].starts_with(b"...");
                    if followed {
                        (T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG, 1)
                    } else {
                        (T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG, 1)
                    }
                }
            },
            b':' => match next {
                Some(b':') => (T_DOUBLE_COLON, 2),
                _ => (i64::from(b':'), 1),
            },
            byte @ (b';' | b',' | b'~' | b'@' | b'(' | b')' | b'[' | b']') => (i64::from(byte), 1),
            _ => (T_BAD_CHARACTER, 1),
        };
        self.emit(id, start, start + len);
        if matches!(id, T_OBJECT_OPERATOR | T_NULLSAFE_OBJECT_OPERATOR) {
            self.push(State::LookingForProperty);
        }
    }

    fn scan_property(&mut self) {
        let src = self.src;
        let start = self.pos;
        let byte = src[start];
        let next = self.byte(start + 1);
        if is_whitespace(byte) {
            let mut end = start;
            while end < src.len() && is_whitespace(src[end]) {
                end += 1;
            }
            self.emit(T_WHITESPACE, start, end);
        } else if byte == b'#' || (byte == b'/' && next == Some(b'/')) {
            self.scan_line_comment(start);
        } else if byte == b'/' && next == Some(b'*') {
            self.scan_block_comment(start);
        } else if src[start..].starts_with(b"?->") {
            self.emit(T_NULLSAFE_OBJECT_OPERATOR, start, start + 3);
        } else if src[start..].starts_with(b"->") {
            self.emit(T_OBJECT_OPERATOR, start, start + 2);
        } else if is_label_start(byte) {
            let end = self.label_end(start);
            self.emit(T_STRING, start, end);
            self.pop();
        } else {
            self.pop();
        }
    }

    fn flush_encapsed(&mut self, start: usize, end: usize) {
        if end > start {
            self.emit(T_ENCAPSED_AND_WHITESPACE, start, end);
        }
    }

    /// Emit the variable at `start` inside an interpolated string and enter
    /// the offset or property state its immediate continuation selects.
    fn scan_encapsed_variable(&mut self, start: usize) {
        let src = self.src;
        let end = self.label_end(start + 1);
        self.emit(T_VARIABLE, start, end);
        let rest = &src[end..];
        if rest.starts_with(b"[") {
            self.push(State::VarOffset);
        } else if (rest.starts_with(b"->") && rest.get(2).is_some_and(|byte| is_label_start(*byte)))
            || (rest.starts_with(b"?->") && rest.get(3).is_some_and(|byte| is_label_start(*byte)))
        {
            self.push(State::LookingForProperty);
        }
    }

    /// Scan one interpolation boundary inside `"…"` or `` `…` ``. Returns
    /// true when a token was produced and the caller must restart.
    fn interpolation_at(&mut self, chunk_start: usize, index: usize) -> bool {
        let src = self.src;
        match src[index] {
            b'$' => match src.get(index + 1) {
                Some(byte) if is_label_start(*byte) => {
                    self.flush_encapsed(chunk_start, index);
                    self.scan_encapsed_variable(index);
                    true
                }
                Some(b'{') => {
                    self.flush_encapsed(chunk_start, index);
                    self.emit(T_DOLLAR_OPEN_CURLY_BRACES, index, index + 2);
                    self.push(State::LookingForVarname);
                    true
                }
                _ => false,
            },
            b'{' if src.get(index + 1) == Some(&b'$') => {
                self.flush_encapsed(chunk_start, index);
                self.emit(T_CURLY_OPEN, index, index + 1);
                self.push(State::Scripting);
                true
            }
            _ => false,
        }
    }

    fn scan_encapsed(&mut self, delimiter: u8) {
        let src = self.src;
        let start = self.pos;
        let mut index = start;
        while index < src.len() {
            let byte = src[index];
            if byte == delimiter {
                self.flush_encapsed(start, index);
                self.emit_char(index);
                self.pop();
                return;
            }
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if self.interpolation_at(start, index) {
                return;
            }
            index += 1;
        }
        self.flush_encapsed(start, src.len());
        self.pos = src.len();
    }

    fn heredoc_closing_end(
        &self,
        line_start: usize,
        label_start: usize,
        label_end: usize,
    ) -> Option<usize> {
        let src = self.src;
        let mut index = line_start;
        while index < src.len() && matches!(src[index], b' ' | b'\t') {
            index += 1;
        }
        let label = &src[label_start..label_end];
        let end = index + label.len();
        (src.len() > end && &src[index..end] == label && !is_label_char(src[end])).then_some(end)
    }

    fn scan_heredoc(&mut self, label_start: usize, label_end: usize, nowdoc: bool) {
        let src = self.src;
        let start = self.pos;
        let mut index = start;
        let mut at_line_start = start == 0 || matches!(src[start - 1], b'\n' | b'\r');
        loop {
            if at_line_start {
                if let Some(end) = self.heredoc_closing_end(index, label_start, label_end) {
                    self.flush_encapsed(start, index);
                    self.emit(T_END_HEREDOC, index, end);
                    self.pop();
                    return;
                }
                at_line_start = false;
            }
            let Some(byte) = src.get(index).copied() else {
                break;
            };
            match byte {
                b'\n' => {
                    index += 1;
                    at_line_start = true;
                }
                b'\r' => {
                    index += 1;
                    if src.get(index) == Some(&b'\n') {
                        index += 1;
                    }
                    at_line_start = true;
                }
                _ if nowdoc => index += 1,
                b'\\' => {
                    index += 1;
                    if src
                        .get(index)
                        .is_some_and(|byte| !matches!(byte, b'\n' | b'\r'))
                    {
                        index += 1;
                    }
                }
                _ => {
                    if self.interpolation_at(start, index) {
                        return;
                    }
                    index += 1;
                }
            }
        }
        self.flush_encapsed(start, src.len());
        self.pos = src.len();
    }

    fn scan_var_offset(&mut self) {
        let src = self.src;
        let start = self.pos;
        let byte = src[start];
        match byte {
            b'0'..=b'9' => {
                let mut end = start;
                if byte == b'0' {
                    let radix: Option<fn(u8) -> bool> = match src.get(start + 1) {
                        Some(b'x') | Some(b'X') => Some(|byte| byte.is_ascii_hexdigit()),
                        Some(b'b') | Some(b'B') => Some(|byte| matches!(byte, b'0' | b'1')),
                        Some(b'o') | Some(b'O') => Some(|byte| matches!(byte, b'0'..=b'7')),
                        _ => None,
                    };
                    if let Some(is_digit) = radix
                        && src.get(start + 2).is_some_and(|byte| is_digit(*byte))
                    {
                        end = digits_end(src, start + 2, is_digit);
                    }
                }
                if end == start {
                    end = digits_end(src, start, |byte| byte.is_ascii_digit());
                }
                self.emit(T_NUM_STRING, start, end);
            }
            b'$' if self.byte(start + 1).is_some_and(is_label_start) => {
                let end = self.label_end(start + 1);
                self.emit(T_VARIABLE, start, end);
            }
            _ if is_label_start(byte) => {
                let end = self.label_end(start);
                self.emit(T_STRING, start, end);
            }
            b']' => {
                self.emit_char(start);
                self.pop();
            }
            b' ' | b'\n' | b'\r' | b'\t' | b'\\' | b'\'' | b'#' => {
                self.emit(T_ENCAPSED_AND_WHITESPACE, start, start);
                self.pop();
            }
            b';' | b':' | b',' | b'.' | b'|' | b'^' | b'&' | b'+' | b'-' | b'/' | b'*' | b'='
            | b'%' | b'!' | b'~' | b'$' | b'<' | b'>' | b'?' | b'@' | b'(' | b')' | b'[' | b'{'
            | b'}' | b'"' | b'`' => self.emit_char(start),
            _ => self.emit(T_BAD_CHARACTER, start, start + 1),
        }
    }

    fn scan_varname(&mut self) {
        let src = self.src;
        let start = self.pos;
        if is_label_start(src[start]) {
            let end = self.label_end(start);
            if matches!(src.get(end), Some(b'[') | Some(b'}')) {
                self.emit(T_STRING_VARNAME, start, end);
            }
        }
        self.states.pop();
        self.push(State::Scripting);
    }
}

/// End of a digit run that may contain single `_` separators between digits.
fn digits_end(src: &[u8], start: usize, is_digit: fn(u8) -> bool) -> usize {
    if !src.get(start).is_some_and(|byte| is_digit(*byte)) {
        return start;
    }
    let mut end = start;
    loop {
        while end < src.len() && is_digit(src[end]) {
            end += 1;
        }
        if src.get(end) == Some(&b'_') && src.get(end + 1).is_some_and(|byte| is_digit(*byte)) {
            end += 1;
            continue;
        }
        return end;
    }
}

fn fits_in_long(digits: &[u8], radix: u32) -> bool {
    let mut value: i64 = 0;
    for byte in digits {
        if *byte == b'_' {
            continue;
        }
        let Some(digit) = (*byte as char).to_digit(radix) else {
            return false;
        };
        let Some(next) = value
            .checked_mul(i64::from(radix))
            .and_then(|value| value.checked_add(i64::from(digit)))
        else {
            return false;
        };
        value = next;
    }
    true
}

/// `TOKEN_PARSE` admits reserved words as identifiers where the grammar does:
/// method, constant and enum-case names, static member access and named
/// arguments. The scanner applies these contextual rules from the token
/// stream itself; it does not run the full grammar.
fn apply_parse_context(tokens: &mut [RawToken]) {
    fn is_reserved(id: i64) -> bool {
        (T_INCLUDE..=T_NS_C).contains(&id)
    }
    let previous = |tokens: &[RawToken], index: usize| -> Option<usize> {
        (0..index)
            .rev()
            .find(|&candidate| !tokens[candidate].is_trivia())
    };
    let next = |tokens: &[RawToken], index: usize| -> Option<usize> {
        (index + 1..tokens.len()).find(|&candidate| !tokens[candidate].is_trivia())
    };
    for index in 0..tokens.len() {
        if !is_reserved(tokens[index].id) {
            continue;
        }
        let Some(before) = previous(tokens, index) else {
            continue;
        };
        let before_id = tokens[before].id;
        let after_id = next(tokens, index).map(|after| tokens[after].id);
        let ampersand = matches!(
            before_id,
            T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG | T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG
        ) && previous(tokens, before)
            .is_some_and(|earlier| tokens[earlier].id == T_FUNCTION);
        let identifier = match before_id {
            T_DOUBLE_COLON | T_FUNCTION | T_CONST => true,
            T_CASE => {
                matches!(after_id, Some(id) if id == i64::from(b'=') || id == i64::from(b';'))
            }
            _ if ampersand => true,
            // Trait adaptation names and named arguments.
            _ => {
                matches!(after_id, Some(T_AS) | Some(T_INSTEADOF))
                    || after_id == Some(i64::from(b':'))
                        && (before_id == i64::from(b'(') || before_id == i64::from(b','))
            }
        };
        if identifier {
            tokens[index].id = T_STRING;
        }
    }
}

/// Report the first front-end failure for `TOKEN_PARSE`.
fn parse_failure(source: &[u8]) -> Option<String> {
    let tokens = crate::lexer::Lexer::new_bytes(source)
        .tokenize_included_source()
        .map_err(|error| error.to_string());
    let message = match tokens {
        Err(message) => message,
        Ok(tokens) => crate::parser::Parser::new(tokens).parse().err()?,
    };
    // The front end appends its own location; ParseError carries the line.
    let trimmed = message
        .rsplit_once(" on line ")
        .filter(|(_, line)| !line.is_empty() && line.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(message.as_str(), |(head, _)| head);
    Some(trimmed.to_string())
}

fn text_value(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(text) => Value::string(text),
        Err(_) => Value::binary_string(bytes),
    }
}

/// Token texts of one tokenization. Single-byte spellings (punctuation, one
/// space, one newline) dominate real sources; they share one string each.
struct TextValues {
    single: Vec<Option<Value>>,
}

impl TextValues {
    fn new() -> Self {
        Self {
            single: vec![None; 256],
        }
    }

    fn value(&mut self, bytes: &[u8]) -> Value {
        if let [byte] = bytes {
            let slot = &mut self.single[usize::from(*byte)];
            if let Some(value) = slot {
                return value.clone();
            }
            let value = text_value(bytes);
            *slot = Some(value.clone());
            return value;
        }
        text_value(bytes)
    }
}

/// Initial property slots and the four token slots of the called class.
struct TokenTemplate {
    class_id: u32,
    layout: std::rc::Rc<ObjectLayout>,
    initial: Vec<Value>,
    slots: [usize; 4],
}

impl TokenTemplate {
    fn capture(object: &Value, class_id: u32) -> Option<Self> {
        let object = object.as_object()?;
        let layout = object.property_layout.clone();
        let slots = [
            layout.slot("id")?,
            layout.slot("text")?,
            layout.slot("line")?,
            layout.slot("pos")?,
        ];
        let initial = (0..layout.len())
            .map(|slot| {
                object
                    .get_property_slot(slot)
                    .cloned()
                    .unwrap_or_else(Value::null)
            })
            .collect();
        Some(Self {
            class_id,
            layout,
            initial,
            slots,
        })
    }

    fn instantiate(&self) -> Value {
        Value::object(crate::value::PhpObject::with_layout(
            self.class_id,
            self.layout.clone(),
            self.initial.clone(),
        ))
    }

    fn fill(&self, object: &Value, id: i64, text: Value, line: usize, pos: usize) {
        let mut object = object.as_object_mut().expect("token instance is an object");
        let values = [
            Value::long(id),
            text,
            Value::long(line as i64),
            Value::long(pos as i64),
        ];
        for (slot, value) in self.slots.iter().zip(values) {
            if let Some(target) = object.get_property_slot_mut(*slot) {
                *target = value;
            }
        }
    }
}

fn argument(ed: *mut ExecuteData, index: u32) -> Value {
    crate::stdlib::owned_argument(ed, index)
}

fn optional_argument(ed: *mut ExecuteData, index: u32) -> Option<Value> {
    let value = argument(ed, index);
    (value.value_type() != ValueType::Undef).then_some(value)
}

fn given_type_name(value: &Value) -> String {
    match value.value_type() {
        ValueType::False => "false".to_string(),
        ValueType::True => "true".to_string(),
        _ => value.diagnostic_type_name().into_owned(),
    }
}

/// Shared argument admission of `token_get_all()` and `PhpToken::tokenize()`.
/// `first` is the CV index of `$code`; diagnostics number parameters from 1.
fn tokenize_arguments(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    function: &str,
    first: u32,
) -> Result<Option<(Vec<u8>, i64)>, VmError> {
    let code = argument(ed, first);
    let Some(code) = super::typed_internal_string_value_expected(
        ed, eg, &code, function, 0, "code", "string", "string",
    )?
    else {
        return Ok(None);
    };
    let flags = match optional_argument(ed, first + 1) {
        Some(flags) => {
            let Some(flags) = super::typed_internal_int_value_argument_expected(
                ed, eg, &flags, function, 1, "flags", "int",
            )?
            else {
                return Ok(None);
            };
            flags
        }
        None => 0,
    };
    let bytes = code
        .php_string_bytes()
        .map(|bytes| bytes.into_owned())
        .unwrap_or_default();
    Ok(Some((bytes, flags)))
}

fn scan_with_flags(eg: &mut ExecutorGlobals, source: &[u8], flags: i64) -> Option<Vec<RawToken>> {
    let mut tokens = scan(source);
    if flags & TOKEN_PARSE != 0 {
        if let Some(message) = parse_failure(source) {
            eg.exception = Some(make_error_value("ParseError", &message));
            return None;
        }
        apply_parse_context(&mut tokens);
    }
    Some(tokens)
}

pub(super) fn token_get_all(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((source, flags)) = tokenize_arguments(ed, eg, "token_get_all", 0)? else {
        return Ok(());
    };
    let Some(tokens) = scan_with_flags(eg, &source, flags) else {
        return Ok(());
    };
    let mut texts = TextValues::new();
    let mut output = PhpArray::with_packed_capacity(tokens.len());
    for token in &tokens {
        let text = texts.value(&source[token.start..token.end]);
        if token.id < T_LNUMBER {
            output.push(text);
        } else {
            let mut fields = PhpArray::with_packed_capacity(3);
            fields.push(Value::long(token.id));
            fields.push(text);
            fields.push(Value::long(token.line as i64));
            output.push(Value::array(fields));
        }
    }
    crate::stdlib::write_return_value(rv, Value::array(output));
    Ok(())
}

pub(super) fn fn_token_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(id) = super::typed_internal_int_argument(ed, eg, "token_name", 0, "id")? else {
        return Ok(());
    };
    crate::stdlib::write_return_value(rv, Value::string(token_name(id).unwrap_or("UNKNOWN")));
    Ok(())
}

// --- PhpToken -------------------------------------------------------------

const PHP_TOKEN: &str = "PhpToken";

fn property_error(eg: &mut ExecutorGlobals, name: &str) {
    eg.exception = Some(make_error_value(
        "Error",
        &format!("Typed property PhpToken::${name} must not be accessed before initialization"),
    ));
}

/// Read one initialized `PhpToken` property or raise PHP's uninitialized
/// typed-property error.
fn token_property(eg: &mut ExecutorGlobals, receiver: &Value, name: &str) -> Option<Value> {
    let value = receiver
        .as_object()
        .and_then(|object| {
            object
                .get_property(name)
                .map(|value| value.dereferenced().clone())
        })
        .filter(|value| value.value_type() != ValueType::Undef);
    if value.is_none() {
        property_error(eg, name);
    }
    value
}

fn set_token_fields(object: &Value, id: i64, text: Value, line: i64, pos: i64) {
    let mut object = object
        .as_object_mut()
        .expect("PhpToken receiver is an object");
    object.set_property("id", Value::long(id));
    object.set_property("text", text);
    object.set_property("line", Value::long(line));
    object.set_property("pos", Value::long(pos));
}

fn php_token_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    const FUNCTION: &str = "PhpToken::__construct";
    let receiver = argument(ed, 0);
    let id = argument(ed, 1);
    let Some(id) =
        super::typed_internal_int_value_argument_expected(ed, eg, &id, FUNCTION, 0, "id", "int")?
    else {
        return Ok(());
    };
    let text = argument(ed, 2);
    let Some(text) = super::typed_internal_string_value_expected(
        ed, eg, &text, FUNCTION, 1, "text", "string", "string",
    )?
    else {
        return Ok(());
    };
    let mut positions = [-1i64, -1];
    for (slot, (index, parameter)) in [(3u32, "line"), (4u32, "pos")].into_iter().enumerate() {
        if let Some(value) = optional_argument(ed, index) {
            let Some(value) = super::typed_internal_int_value_argument_expected(
                ed,
                eg,
                &value,
                FUNCTION,
                index - 1,
                parameter,
                "int",
            )?
            else {
                return Ok(());
            };
            positions[slot] = value;
        }
    }
    set_token_fields(&receiver, id, text, positions[0], positions[1]);
    Ok(())
}

fn php_token_tokenize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((source, flags)) = tokenize_arguments(ed, eg, "PhpToken::tokenize", 1)? else {
        return Ok(());
    };
    let class_name = crate::vm::execute::called_class_name_for_internal_call(eg, ed)
        .unwrap_or(PHP_TOKEN)
        .to_string();
    let class_id = eg.class_id_of(&class_name);
    let Some(tokens) = scan_with_flags(eg, &source, flags) else {
        return Ok(());
    };
    let mut texts = TextValues::new();
    let mut output = PhpArray::with_packed_capacity(tokens.len());
    // The first instance resolves the called class's defaults through the
    // ordinary instantiation path; later instances copy its initial slots.
    let mut template: Option<TokenTemplate> = None;
    for token in &tokens {
        let text = texts.value(&source[token.start..token.end]);
        let object = match &template {
            Some(template) => template.instantiate(),
            None => {
                let Some(object) =
                    crate::vm::execute::instantiate_protocol_object(eg, ed, class_id)?
                else {
                    return Ok(());
                };
                template = TokenTemplate::capture(&object, class_id);
                object
            }
        };
        match &template {
            Some(template) => template.fill(&object, token.id, text, token.line, token.start),
            None => set_token_fields(
                &object,
                token.id,
                text,
                token.line as i64,
                token.start as i64,
            ),
        }
        output.push(object);
    }
    crate::stdlib::write_return_value(rv, Value::array(output));
    Ok(())
}

fn php_token_is(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = argument(ed, 0);
    let kind = argument(ed, 1);
    let kind = kind.dereferenced();
    let matches_one = |eg: &mut ExecutorGlobals, candidate: &Value| -> Option<bool> {
        match candidate.value_type() {
            ValueType::Long => {
                let id = token_property(eg, &receiver, "id")?;
                Some(id.as_long() == candidate.as_long())
            }
            ValueType::String => {
                let text = token_property(eg, &receiver, "text")?;
                Some(text.php_string_bytes() == candidate.php_string_bytes())
            }
            _ => None,
        }
    };
    let result = match kind.value_type() {
        ValueType::Long | ValueType::String => matches_one(eg, kind),
        ValueType::Array => {
            let elements: Vec<Value> = kind
                .as_array()
                .map(|array| {
                    array
                        .values()
                        .map(|value| value.dereferenced().clone())
                        .collect()
                })
                .unwrap_or_default();
            let mut found = Some(false);
            for element in &elements {
                if !matches!(element.value_type(), ValueType::Long | ValueType::String) {
                    eg.exception = Some(make_error_value(
                        "TypeError",
                        &format!(
                            "PhpToken::is(): Argument #1 ($kind) must only have elements of type string|int, {}",
                            given_type_name(element) + " given"
                        ),
                    ));
                    return Ok(());
                }
                match matches_one(eg, element) {
                    Some(true) => {
                        found = Some(true);
                        break;
                    }
                    Some(false) => {}
                    None => {
                        found = None;
                        break;
                    }
                }
            }
            found
        }
        _ => {
            eg.exception = Some(make_error_value(
                "TypeError",
                &format!(
                    "PhpToken::is(): Argument #1 ($kind) must be of type string|int|array, {} given",
                    given_type_name(kind)
                ),
            ));
            return Ok(());
        }
    };
    if let Some(result) = result {
        crate::stdlib::write_return_value(rv, Value::bool(result));
    }
    Ok(())
}

fn php_token_is_ignorable(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = argument(ed, 0);
    let Some(id) = token_property(eg, &receiver, "id") else {
        return Ok(());
    };
    let ignorable = matches!(
        id.as_long(),
        Some(T_WHITESPACE) | Some(T_COMMENT) | Some(T_DOC_COMMENT) | Some(T_OPEN_TAG)
    );
    crate::stdlib::write_return_value(rv, Value::bool(ignorable));
    Ok(())
}

fn php_token_get_token_name(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = argument(ed, 0);
    let Some(id) = token_property(eg, &receiver, "id") else {
        return Ok(());
    };
    let id = id.as_long().unwrap_or(0);
    let name = if (0..256).contains(&id) {
        Some(text_value(&[id as u8]))
    } else {
        token_name(id).map(Value::string)
    };
    crate::stdlib::write_return_value(rv, name.unwrap_or_else(Value::null));
    Ok(())
}

fn php_token_to_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let receiver = argument(ed, 0);
    let Some(text) = token_property(eg, &receiver, "text") else {
        return Ok(());
    };
    crate::stdlib::write_return_value(rv, text);
    Ok(())
}

fn token_property_definition(name: &str, hint: ParamTypeHint) -> PropertyDefinition {
    PropertyDefinition::declared(
        name.to_string(),
        None,
        Visibility::Public,
        PHP_TOKEN.to_string(),
        hint,
        false,
        false,
    )
}

struct MethodDeclaration {
    name: &'static str,
    handler: crate::vm::function::InternalFunctionHandler,
    parameters: &'static [&'static str],
    required: u32,
    hints: fn() -> Vec<ParamTypeHint>,
    return_hint: fn() -> ParamTypeHint,
    defaults: fn() -> Vec<Option<Value>>,
    /// Reflection spellings of the optional parameter defaults.
    default_spellings: &'static [Option<&'static str>],
    is_static: bool,
}

/// Register the `PhpToken` class. Tokenizer functions are registered with
/// the ordinary function registry; both share this extension identity.
pub(super) fn register_classes(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: PHP_TOKEN.to_string(),
        source_file: None,
        declaration_line: 0,
        parent: None,
        implements: vec!["Stringable".to_string()],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties: vec![
            token_property_definition("id", ParamTypeHint::Int),
            token_property_definition("text", ParamTypeHint::String),
            token_property_definition("line", ParamTypeHint::Int),
            token_property_definition("pos", ParamTypeHint::Int),
        ],
        static_properties: Vec::new(),
        constants: Vec::new(),
        property_layout: std::rc::Rc::new(ObjectLayout::empty()),
        property_defaults: std::rc::Rc::from([]),
        readonly_props: Vec::new(),
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .expect("PhpToken registers once per request");

    let declarations = [
        MethodDeclaration {
            name: "tokenize",
            handler: php_token_tokenize,
            parameters: &["code", "flags"],
            required: 1,
            hints: || vec![ParamTypeHint::String, ParamTypeHint::Int],
            return_hint: || ParamTypeHint::Array,
            defaults: || vec![None, Some(Value::long(0))],
            default_spellings: &[None, Some("0")],
            is_static: true,
        },
        MethodDeclaration {
            name: "__construct",
            handler: php_token_construct,
            parameters: &["id", "text", "line", "pos"],
            required: 2,
            hints: || {
                vec![
                    ParamTypeHint::Int,
                    ParamTypeHint::String,
                    ParamTypeHint::Int,
                    ParamTypeHint::Int,
                ]
            },
            return_hint: || ParamTypeHint::None,
            defaults: || vec![None, None, Some(Value::long(-1)), Some(Value::long(-1))],
            default_spellings: &[None, None, Some("-1"), Some("-1")],
            is_static: false,
        },
        MethodDeclaration {
            name: "is",
            handler: php_token_is,
            parameters: &["kind"],
            required: 1,
            hints: || {
                vec![ParamTypeHint::Union(vec![
                    ParamTypeHint::Array,
                    ParamTypeHint::String,
                    ParamTypeHint::Int,
                ])]
            },
            return_hint: || ParamTypeHint::Bool,
            defaults: || vec![None],
            default_spellings: &[None],
            is_static: false,
        },
        MethodDeclaration {
            name: "isIgnorable",
            handler: php_token_is_ignorable,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            return_hint: || ParamTypeHint::Bool,
            defaults: Vec::new,
            default_spellings: &[],
            is_static: false,
        },
        MethodDeclaration {
            name: "getTokenName",
            handler: php_token_get_token_name,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            return_hint: || ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
            defaults: Vec::new,
            default_spellings: &[],
            is_static: false,
        },
        MethodDeclaration {
            name: "__toString",
            handler: php_token_to_string,
            parameters: &[],
            required: 0,
            hints: Vec::new,
            return_hint: || ParamTypeHint::String,
            defaults: Vec::new,
            default_spellings: &[],
            is_static: false,
        },
    ];

    let mut functions = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let hints = (declaration.hints)();
        eg.register_internal_method_contract(
            PHP_TOKEN,
            declaration.name,
            declaration.is_static,
            declaration.required,
            declaration.parameters,
            hints.clone(),
            (declaration.return_hint)(),
            declaration.default_spellings,
            false,
        );
        let mut function = Box::new(
            make_internal_method(
                declaration.handler,
                declaration.parameters.len() as u32 + 1,
                declaration.required,
                Vec::new(),
            )
            .with_static_parameter_names(declaration.parameters),
        );
        // Every handler admits its own arguments with PHP's parameter
        // numbering; the hints remain available to Reflection and linking.
        function.handler_validates_types = true;
        function.common.sig.param_type_hints = hints;
        function.common.sig.return_type_hint = (declaration.return_hint)();
        let pointer = &function.common as *const FunctionCommon;
        eg.function_table.insert(
            super::builtin_classes::internal_method_lookup_name(PHP_TOKEN, declaration.name),
            pointer,
        );
        eg.method_declaring_class.insert(pointer, PHP_TOKEN.into());
        if declaration.is_static {
            eg.register_internal_static_method(pointer);
        }
        eg.register_internal_function_display_name(
            pointer,
            super::builtin_classes::internal_method_display_name(PHP_TOKEN, declaration.name),
        );
        eg.register_internal_function_reflection_metadata(
            pointer,
            (declaration.defaults)(),
            "tokenizer",
        );
        functions.push(function);
    }
    eg.mark_internal_method_final(PHP_TOKEN, "__construct");
    functions
}

/// Register `token_get_all()` and `token_name()`.
pub(super) fn register_functions(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions = Vec::with_capacity(2);
    let mut get_all = Box::new(
        make_internal_function(token_get_all, 2, 1, Vec::new())
            .with_static_parameter_names(&["code", "flags"]),
    );
    get_all.common.sig.param_type_hints = vec![ParamTypeHint::String, ParamTypeHint::Int];
    get_all.common.sig.return_type_hint = ParamTypeHint::Array;
    get_all.handler_validates_types = true;
    let pointer = &get_all.common as *const FunctionCommon;
    eg.register_function("token_get_all", pointer)
        .expect("token_get_all registers once per request");
    eg.register_internal_function_reflection_metadata(
        pointer,
        vec![None, Some(Value::long(0))],
        "tokenizer",
    );
    functions.push(get_all);

    let mut name = Box::new(
        make_internal_function(fn_token_name, 1, 1, Vec::new())
            .with_static_parameter_names(&["id"]),
    );
    name.common.sig.param_type_hints = vec![ParamTypeHint::Int];
    name.common.sig.return_type_hint = ParamTypeHint::String;
    name.handler_validates_types = true;
    let pointer = &name.common as *const FunctionCommon;
    eg.register_function("token_name", pointer)
        .expect("token_name registers once per request");
    eg.register_internal_function_reflection_metadata(pointer, vec![None], "tokenizer");
    functions.push(name);
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(source: &str) -> Vec<(i64, &str, usize)> {
        scan(source.as_bytes())
            .into_iter()
            .map(|token| (token.id, &source[token.start..token.end], token.line))
            .collect()
    }

    #[test]
    fn rejoined_tokens_reproduce_the_source() {
        let source = "html<?php\n/** docs */ namespace Rphp\\Fixture; // comment\nnew class {}; Fixture::class; \"a{$b->c}d $e[0]\" <<<X\n  y\n  X;\n__halt_compiler(); tail";
        let rebuilt: String = dump(source).into_iter().map(|(_, text, _)| text).collect();
        assert_eq!(rebuilt, source);
    }

    #[test]
    fn keywords_names_and_operators_have_public_identities() {
        let tokens = dump(
            "<?php namespace\\a; \\b\\c; d\\e; function &f() {} yield from g(); $h?->i; #[J] $k |> l(...);",
        );
        let ids: Vec<i64> = tokens
            .iter()
            .map(|(id, _, _)| *id)
            .filter(|id| *id != T_WHITESPACE)
            .collect();
        assert_eq!(
            ids,
            vec![
                T_OPEN_TAG,
                T_NAME_RELATIVE,
                i64::from(b';'),
                T_NAME_FULLY_QUALIFIED,
                i64::from(b';'),
                T_NAME_QUALIFIED,
                i64::from(b';'),
                T_FUNCTION,
                T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG,
                T_STRING,
                i64::from(b'('),
                i64::from(b')'),
                i64::from(b'{'),
                i64::from(b'}'),
                T_YIELD_FROM,
                T_STRING,
                i64::from(b'('),
                i64::from(b')'),
                i64::from(b';'),
                T_VARIABLE,
                T_NULLSAFE_OBJECT_OPERATOR,
                T_STRING,
                i64::from(b';'),
                T_ATTRIBUTE,
                T_STRING,
                i64::from(b']'),
                T_VARIABLE,
                T_PIPE,
                T_STRING,
                i64::from(b'('),
                T_ELLIPSIS,
                i64::from(b')'),
                i64::from(b';'),
            ]
        );
    }

    #[test]
    fn interpolated_strings_split_into_public_parts() {
        let tokens = dump("<?php \"$a[0] {$b} ${c}\";");
        let shape: Vec<(i64, &str)> = tokens.iter().map(|(id, text, _)| (*id, *text)).collect();
        assert_eq!(
            shape,
            vec![
                (T_OPEN_TAG, "<?php "),
                (i64::from(b'"'), "\""),
                (T_VARIABLE, "$a"),
                (i64::from(b'['), "["),
                (T_NUM_STRING, "0"),
                (i64::from(b']'), "]"),
                (T_ENCAPSED_AND_WHITESPACE, " "),
                (T_CURLY_OPEN, "{"),
                (T_VARIABLE, "$b"),
                (i64::from(b'}'), "}"),
                (T_ENCAPSED_AND_WHITESPACE, " "),
                (T_DOLLAR_OPEN_CURLY_BRACES, "${"),
                (T_STRING_VARNAME, "c"),
                (i64::from(b'}'), "}"),
                (i64::from(b'"'), "\""),
                (i64::from(b';'), ";"),
            ]
        );
    }

    #[test]
    fn heredoc_bodies_and_line_numbers_follow_php() {
        let tokens = dump("<?php $x = <<<EOT\r\n  a $b\r\n    EOT;\n$y;");
        let lines: Vec<(i64, usize)> = tokens.iter().map(|(id, _, line)| (*id, *line)).collect();
        assert_eq!(
            lines,
            vec![
                (T_OPEN_TAG, 1),
                (T_VARIABLE, 1),
                (T_WHITESPACE, 1),
                (i64::from(b'='), 1),
                (T_WHITESPACE, 1),
                (T_START_HEREDOC, 1),
                (T_ENCAPSED_AND_WHITESPACE, 2),
                (T_VARIABLE, 2),
                (T_ENCAPSED_AND_WHITESPACE, 2),
                (T_END_HEREDOC, 3),
                (i64::from(b';'), 3),
                (T_WHITESPACE, 3),
                (T_VARIABLE, 4),
                (i64::from(b';'), 4),
            ]
        );
        assert_eq!(tokens[9].1, "    EOT");
    }

    #[test]
    fn halt_compiler_publishes_the_tail_as_inline_html() {
        let tokens = dump("<?php __halt_compiler /*c*/ ( ) ; ?>\nrest");
        let last = tokens.last().unwrap();
        assert_eq!((last.0, last.1), (T_INLINE_HTML, " ?>\nrest"));
    }

    #[test]
    fn numeric_literals_classify_overflow_and_separators() {
        let ids: Vec<(i64, &str)> = dump("<?php 1_000 0x8000000000000000 09 1..2 .5_5 0o17 1e 077")
            .into_iter()
            .filter(|(id, _, _)| *id != T_WHITESPACE && *id != T_OPEN_TAG)
            .map(|(id, text, _)| (id, text))
            .collect();
        assert_eq!(
            ids,
            vec![
                (T_LNUMBER, "1_000"),
                (T_DNUMBER, "0x8000000000000000"),
                (T_LNUMBER, "09"),
                (T_DNUMBER, "1."),
                (T_DNUMBER, ".2"),
                (T_DNUMBER, ".5_5"),
                (T_LNUMBER, "0o17"),
                (T_LNUMBER, "1"),
                (T_STRING, "e"),
                (T_LNUMBER, "077"),
            ]
        );
    }

    #[test]
    fn parse_flag_admits_reserved_words_as_identifiers() {
        let source = "<?php class A { function list() {} const NEW = 1; } A::class; f(list: 1);";
        let mut tokens = scan(source.as_bytes());
        apply_parse_context(&mut tokens);
        let strings: Vec<&str> = tokens
            .iter()
            .filter(|token| token.id == T_STRING)
            .map(|token| &source[token.start..token.end])
            .collect();
        assert_eq!(strings, vec!["A", "list", "NEW", "A", "class", "f", "list"]);
    }

    #[test]
    #[ignore = "manual throughput probe: RPHP_TOKENIZER_BENCH_FILE=path cargo test --release -- --ignored --nocapture"]
    fn scanner_throughput_probe() {
        let Ok(path) = std::env::var("RPHP_TOKENIZER_BENCH_FILE") else {
            return;
        };
        let source = std::fs::read(path).unwrap();
        let started = std::time::Instant::now();
        let mut tokens = 0;
        for _ in 0..5 {
            tokens += scan(&source).len();
        }
        let elapsed = started.elapsed().as_secs_f64();
        eprintln!(
            "scan: {tokens} tokens in {elapsed:.3}s ({:.1} MB/s)",
            source.len() as f64 * 5.0 / elapsed / 1e6
        );
    }

    #[test]
    fn constant_table_and_names_agree() {
        assert_eq!(constant_value("T_PAAMAYIM_NEKUDOTAYIM"), Some(402));
        assert_eq!(token_name(402), Some("T_DOUBLE_COLON"));
        assert_eq!(token_name(59), None);
        assert_eq!(token_name(T_BAD_CHARACTER), Some("T_BAD_CHARACTER"));
    }
}
