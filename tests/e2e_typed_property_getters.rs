mod common;
use common::run_php;
macro_rules! contract {
    ($name:ident) => {
        #[test]
        fn $name() {
            assert_eq!(
                run_php(include_str!(concat!(
                    "fixtures/typed_property_getters/",
                    stringify!($name),
                    ".php"
                ))),
                include_str!(concat!(
                    "fixtures/typed_property_getters/",
                    stringify!($name),
                    ".out"
                )),
            );
        }
    };
}
contract!(exact_types);
contract!(nullable_relative);
contract!(reference_cow);
contract!(coercion_composed);
contract!(uninitialized_magic);
contract!(lazy_hooks);
contract!(diagnostics_extra_args);
