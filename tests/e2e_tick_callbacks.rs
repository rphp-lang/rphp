mod common;

#[test]
fn lexical_ticks_count_echo_operands_and_loop_boundaries() {
    assert_eq!(
        common::run_php(
            r#"<?php
        $events=0;
        function pulse(){echo 'T',++$GLOBALS['events'],';';}
        register_tick_function('pulse'); echo 'plain;';
        declare(ticks=1) {
            echo 'one;'; $value=8; $value; ;
            if(false){echo 'dead;';} if(true){echo 'branch;';}
            for($i=0;$i<2;$i++){echo 'loop',$i,';';}
            while(false){echo 'never;';}
            foreach([2,4] as $part){echo 'item',$part,';';}
            echo 'last;';
        }
        echo 'outside;'; unregister_tick_function('pulse'); echo 'count:',$events,"\n";
    "#
        ),
        "plain;one;T1;T2;branch;T3;loopT4;0T5;;T6;loopT7;1T8;;T9;T10;T11;itemT12;2T13;;T14;itemT15;4T16;;T17;T18;last;T19;outside;count:19\n"
    );
}

#[test]
fn nested_intervals_restore_lexical_setting_and_share_counter() {
    assert_eq!(
        common::run_php(
            r#"<?php
        $events=0; function pulse(){echo 'T',++$GLOBALS['events'],';';}
        register_tick_function('pulse');
        declare(ticks=3) {
            echo 'a;';echo 'b;';echo 'c;';echo 'd;';
            declare(ticks=0){echo 'disabled;';echo 'ignored;';}
            echo 'e;';declare(ticks=2){echo 'f;';echo 'g;';}echo 'h;';echo 'i;';
        }
        echo 'outside;';declare(ticks=3){echo 'j;';echo 'k;';}
        unregister_tick_function('pulse');echo 'count:',$events,"\n";
    "#
        ),
        "a;b;c;T1;d;disabled;ignored;e;T2;f;g;T3;h;i;outside;j;T4;k;count:4\n"
    );
}

#[test]
fn functions_closures_methods_inherit_definition_not_caller_ticks() {
    assert_eq!(
        common::run_php(
            r#"<?php
        function plain(){echo 'plain;';}
        declare(ticks=1){
            function enabled(){echo 'enabled;';return 'return;';}
            $closure=function(){echo 'closure;';};
            class TickMethod {function run(){echo 'method;';}}
        }
        function pulse(){echo 'T;';}register_tick_function('pulse');
        plain();echo enabled();$closure();(new TickMethod)->run();
        declare(ticks=1){plain();}unregister_tick_function('pulse');
    "#
        ),
        "plain;enabled;T;return;closure;T;method;T;plain;T;"
    );
}

#[test]
fn duplicate_callbacks_are_distinct_entries_and_remove_one_at_a_time() {
    assert_eq!(
        common::run_php(
            r#"<?php
        $p=function($x=0){echo $x,';';};
        var_dump(register_tick_function($p));register_tick_function($p);
        register_tick_function($p,1);register_tick_function($p,1);
        declare(ticks=1){echo 'body;';}var_dump(unregister_tick_function($p));
        declare(ticks=1){echo 'next;';}
    "#
        ),
        "bool(true)\nbody;0;0;1;1;NULL\nnext;0;1;1;"
    );
}

#[test]
fn registry_walk_is_live_and_reentry_does_not_reinvoke_callbacks() {
    assert_eq!(
        common::run_php(
            r#"<?php
        declare(ticks=1){function added(){echo 'added;';}}
        function initial(){echo 'initial;';static $once=false;if(!$once){$once=true;register_tick_function('added');}}
        register_tick_function('initial');
        declare(ticks=1){echo 'first;';echo 'second;';}
    "#
        ),
        "first;initial;added;second;initial;added;"
    );
}

#[test]
fn active_callback_cannot_remove_itself_but_can_remove_later_entry() {
    assert_eq!(
        common::run_php(
            r#"<?php
        function first(){echo 'first;';try{unregister_tick_function('FIRST');}
            catch(Error $e){echo $e->getMessage(),';';}unregister_tick_function('second');}
        function second(){echo 'second;';}
        register_tick_function('first');register_tick_function('second');
        declare(ticks=1){echo 'body;';}unregister_tick_function('first');echo 'done';
    "#
        ),
        "body;first;Registered tick function cannot be unregistered while it is being executed;done"
    );
}

#[test]
fn escaping_tick_exception_stops_statement_and_clears_reentry_guard() {
    assert_eq!(
        common::run_php(
            r#"<?php
        $p=function(){echo 'callback;';throw new RuntimeException('tick-stop');};
        register_tick_function($p);
        try{declare(ticks=1){echo 'before;';echo 'unreachable;';}}
        catch(Throwable $e){echo $e->getMessage(),';';}
        unregister_tick_function($p);declare(ticks=1){echo 'after;';}echo 'done';
    "#
        ),
        "before;callback;tick-stop;after;done"
    );
}

#[test]
fn registration_retains_private_scope_magic_name_and_value_arguments() {
    assert_eq!(
        common::run_php(
            r#"<?php
        class TickOwner{
            private static function hidden($x){echo $x,';';}
            public static function install(){register_tick_function([self::class,'hidden'],'private');}
            public function __call($n,$args){echo $n,':',implode(',',$args),';';}
        }
        TickOwner::install();$o=new TickOwner;register_tick_function([$o,'absent'],'magic');
        declare(ticks=1){echo 'body;';}
    "#
        ),
        "body;private;absent:magic;"
    );
}

#[test]
fn tick_declaration_literals_reject_nonliteral_before_execution() {
    for value in [
        "true", "false", "null", "-1", "-0", "(-0)", "+1", "1+1", "INTERVAL", "[]",
    ] {
        let source = format!("<?php echo 'unreachable'; if(false){{declare(ticks={value}){{}}}}");
        let tokens = rphp::lexer::Lexer::new(&source).tokenize().unwrap();
        let statements = rphp::parser::Parser::new(tokens).parse().unwrap();
        let error = rphp::compiler::compile::Compiler::new()
            .compile(&statements)
            .err()
            .unwrap_or_else(|| panic!("{value} must fail at compilation"));
        assert!(
            error.contains("declare(ticks) value must be a literal"),
            "{value}: {error}"
        );
    }
}

#[test]
fn disabled_regions_emit_no_tick_bytecode() {
    let source = "<?php $x=1; echo $x; while(false){}";
    let tokens = rphp::lexer::Lexer::new(source).tokenize().unwrap();
    let statements = rphp::parser::Parser::new(tokens).parse().unwrap();
    let result = rphp::compiler::compile::Compiler::new()
        .compile(&statements)
        .unwrap();
    assert!(!format!("{:?}", result.main.instructions).contains("Tick"));
}

#[test]
fn duplicate_active_removal_retains_current_and_removes_next_matching_entry() {
    assert_eq!(
        common::run_php(
            r#"<?php
        function p(){echo 'p;';try{unregister_tick_function('p');}catch(Error $e){echo 'blocked;';}}
        register_tick_function('p');register_tick_function('p');register_tick_function('p');
        declare(ticks=1){echo 'one;';}echo 'outside;';unregister_tick_function('p');
        declare(ticks=1){echo 'two;';}
    "#
        ),
        "one;p;blocked;p;outside;two;"
    );
}

#[test]
fn callback_arguments_remain_value_snapshots_and_warn_for_reference_parameters() {
    assert_eq!(
        common::run_php(
            r#"<?php
        $v=1;$a=[2];function argsPulse(&$v,$a){$v++;echo json_encode([$v,$a]),';';}
        set_error_handler(function($n,$s){echo $s,';';});
        register_tick_function('argsPulse',$v,$a);$v=9;$a[0]=8;
        declare(ticks=1){echo 'body;';echo 'again;';}unregister_tick_function('argsPulse');echo $v;
    "#
        ),
        "body;argsPulse(): Argument #1 ($v) must be passed by reference, value given;[2,[2]];again;argsPulse(): Argument #1 ($v) must be passed by reference, value given;[2,[2]];9"
    );
}

#[test]
fn callback_ticks_advance_shared_counter_without_recursive_dispatch() {
    assert_eq!(
        common::run_php(
            r#"<?php
        declare(ticks=5){function p(){echo 'cb;';}}register_tick_function('p');
        declare(ticks=3){echo '1;';echo '2;';echo '3;';echo '4;';echo '5;';echo '6;';}
    "#
        ),
        "1;2;cb;3;4;cb;5;6;cb;"
    );
}

#[test]
fn registry_owns_invokable_until_request_shutdown() {
    assert_eq!(
        common::run_php(
            r#"<?php
        class TickLease{function __invoke(){echo 'call;';}function __destruct(){echo 'drop;';}}
        $p=new TickLease;register_tick_function($p);unset($p);
        declare(ticks=1){echo 'body;';}echo 'end;';
    "#
        ),
        "body;call;end;drop;"
    );
}

#[test]
fn literal_parentheses_and_alternative_declarations_keep_scope() {
    assert_eq!(
        common::run_php(
            r#"<?php
        function p(){echo 'T;';}register_tick_function('p');
        declare(ticks=(1)) echo 'single;';echo 'outside;';
        declare(ticks=1):echo 'colon;';enddeclare;echo 'end;';
    "#
        ),
        "single;T;outside;colon;T;end;"
    );
}

#[test]
fn empty_control_boundaries_and_explicit_transfers_do_not_add_phantom_ticks() {
    assert_eq!(
        common::run_php(
            r#"<?php
        function p(){echo 'T;';}register_tick_function('p');
        declare(ticks=1){switch(1){}echo 'between;';switch(0){case 0:echo 'case;';break;}
            for($i=0;$i<2;$i++){if($i===0)continue;echo 'loop;';break;}echo 'end;';}
    "#
        ),
        "T;between;T;case;T;loop;T;T;end;T;"
    );
}

#[test]
fn literal_numeric_strings_and_wrapped_intervals_use_php_integer_conversion() {
    for (literal, expected) in [
        ("'1e1'", "a;b;c;d;T;e;f;"),
        ("'2tail'", "a;b;T;c;d;e;T;f;"),
        ("'invalid'", "a;b;c;d;e;f;T;"),
        ("1.9", "a;T;b;T;c;T;d;e;f;T;"),
        ("4294967297", "a;T;b;T;c;T;d;e;f;T;"),
    ] {
        let source = format!(
            "<?php function p(){{echo 'T;';}}register_tick_function('p');
            declare(ticks={literal}){{echo 'a;';echo 'b;';echo 'c;';}}
            declare(ticks=3){{echo 'd;';echo 'e;';echo 'f;';}}"
        );
        assert_eq!(common::run_php(&source), expected, "{literal}");
    }
}

#[test]
fn unrepresentable_float_interval_retains_source_unit_warning() {
    let tokens = rphp::lexer::Lexer::new("<?php if(false){declare(ticks=1e100){}}")
        .tokenize()
        .unwrap();
    let statements = rphp::parser::Parser::new(tokens).parse().unwrap();
    assert!(statements.iter().any(|statement| matches!(statement,
        rphp::parser::Stmt::ExprStmt(rphp::parser::Expr::CompileWarning { message, .. })
        if message == "The float 1.0E+100 is not representable as an int, cast occurred")));
}

#[test]
fn invalid_single_declare_body_preserves_internal_diagnostic_boundary() {
    for (punctuation, token) in [
        ("?", "Question"),
        ("]", "RBracket"),
        (")", "RParen"),
        ("*", "Star"),
    ] {
        let source = format!("<?php declare(ticks=1) {punctuation}");
        let tokens = rphp::lexer::Lexer::new(&source).tokenize().unwrap();
        assert_eq!(
            rphp::parser::Parser::new(tokens).parse().unwrap_err(),
            format!("Expected Semicolon, got {token}")
        );
    }
    let tokens = rphp::lexer::Lexer::new("<?php declare(ticks=1) echo ?;")
        .tokenize()
        .unwrap();
    let error = rphp::parser::Parser::new(tokens).parse().unwrap_err();
    assert!(!error.starts_with("Expected Semicolon"), "{error}");
}
