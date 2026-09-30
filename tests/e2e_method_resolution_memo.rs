mod common;

use common::run_php;

macro_rules! contract {
    ($name:ident) => {
        #[test]
        fn $name() {
            assert_eq!(
                run_php(include_str!(concat!(
                    "fixtures/method_resolution_memo/",
                    stringify!($name),
                    ".php"
                ))),
                include_str!(concat!(
                    "fixtures/method_resolution_memo/",
                    stringify!($name),
                    ".out"
                )),
            );
        }
    };
}

contract!(alternating);
contract!(private_owner);
contract!(rebound_scope);
contract!(reference_return);
contract!(stateful_glob);
contract!(static_magic);
