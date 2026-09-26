//! Native random engines. State is private, independently cloned and never
//! represented by writable PHP properties. Integer seeding uses SplitMix64;
//! xoshiro256** uses its published 256-bit linear transition and jump polynomials.
use super::*;
use crate::value::NativeObjectState;
use crate::vm::function::InternalFunctionHandler;

const ENGINE: &str = "Random\\Engine";
const CRYPTO: &str = "Random\\CryptoSafeEngine";
const SECURE: &str = "Random\\Engine\\Secure";
const XOSHIRO: &str = "Random\\Engine\\Xoshiro256StarStar";

#[derive(Clone, Default)]
struct State([u64; 4]);
impl NativeObjectState for State {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
impl State {
    fn integer(mut seed: u64) -> Self {
        let mut words = [0; 4];
        for word in &mut words {
            seed = seed.wrapping_add(0x9e3779b97f4a7c15);
            let mut mixed = (seed ^ (seed >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d049bb133111eb);
            *word = mixed ^ (mixed >> 31);
        }
        Self(words)
    }
    fn bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 32 {
            return None;
        }
        let mut words = [0; 4];
        for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(8)) {
            *word = u64::from_le_bytes(chunk.try_into().ok()?);
        }
        words.iter().any(|word| *word != 0).then_some(Self(words))
    }
    fn next(&mut self) -> [u8; 8] {
        let output = self.0[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let shifted = self.0[1] << 17;
        self.0[2] ^= self.0[0];
        self.0[3] ^= self.0[1];
        self.0[1] ^= self.0[2];
        self.0[0] ^= self.0[3];
        self.0[2] ^= shifted;
        self.0[3] = self.0[3].rotate_left(45);
        output.to_le_bytes()
    }
    fn jump(&mut self, polynomial: [u64; 4]) {
        let mut result = [0; 4];
        for word in polynomial {
            for bit in 0..64 {
                if word & (1 << bit) != 0 {
                    for (to, from) in result.iter_mut().zip(self.0) {
                        *to ^= from;
                    }
                }
                self.next();
            }
        }
        self.0 = result;
    }
    fn projection(&self) -> Value {
        let mut values = PhpArray::with_packed_capacity(4);
        for word in self.0 {
            values.push(Value::string(format!("{:016x}", word.swap_bytes())));
        }
        Value::array(values)
    }
}

fn entropy<const N: usize>(eg: &mut ExecutorGlobals) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut input| input.read_exact(&mut bytes))
        .is_err()
    {
        eg.exception = Some(make_error_value(
            "Random\\RandomException",
            "Could not gather sufficient random data",
        ));
        None
    } else {
        Some(bytes)
    }
}

fn construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // The full internal ABI validates/coerces the public union before entry.
    let seed = arg_opt!(ed, 1).cloned().unwrap_or_else(Value::null);
    let state = match seed.value_type() {
        ValueType::Null => loop {
            let Some(bytes) = entropy::<32>(eg) else {
                break None;
            };
            if let Some(state) = State::bytes(&bytes) {
                break Some(state);
            }
        },
        ValueType::Long => Some(State::integer(seed.as_long().unwrap() as u64)),
        ValueType::String => {
            let bytes = seed.php_string_bytes().unwrap();
            if bytes.len() != 32 {
                eg.exception = Some(make_error_value(
                    "ValueError",
                    &format!(
                        "{XOSHIRO}::__construct(): Argument #1 ($seed) must be a 32 byte (256 bit) string"
                    ),
                ));
                None
            } else if let Some(state) = State::bytes(&bytes) {
                Some(state)
            } else {
                eg.exception = Some(make_error_value(
                    "ValueError",
                    &format!(
                        "{XOSHIRO}::__construct(): Argument #1 ($seed) must not consist entirely of NUL bytes"
                    ),
                ));
                None
            }
        }
        _ => unreachable!("validated random seed"),
    };
    if let Some(state) = state {
        *arg!(ed, 0)
            .as_object_mut()
            .unwrap()
            .native_object_state_mut::<State>() = state;
    }
    Ok(())
}
fn generate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let bytes = arg!(ed, 0)
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<State>()
        .next();
    ret!(rv, Value::binary_string(&bytes));
}
fn secure(_ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    if let Some(bytes) = entropy::<8>(eg) {
        ret!(rv, Value::binary_string(&bytes));
    }
    Ok(())
}
fn jump(ed: *mut ExecuteData, _rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    arg!(ed, 0)
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<State>()
        .jump([
            0x180ec6d33cfd0aba,
            0xd5a61266f0c9392c,
            0xa9582618e03fc9aa,
            0x39abdc4529b1661c,
        ]);
    Ok(())
}
fn jump_long(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    arg!(ed, 0)
        .as_object_mut()
        .unwrap()
        .native_object_state_mut::<State>()
        .jump([
            0x76e15d3efefdcbbf,
            0xc5004e441c522fb3,
            0x77710069854ee241,
            0x39109bb02acbe635,
        ]);
    Ok(())
}
pub(in crate::stdlib) fn debug_projection(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    let state = object.native_object_state::<State>()?;
    let mut output = PhpArray::new();
    output.set_str("__states", state.projection());
    Some(Value::array(output))
}
fn debug_info(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    ret!(
        rv,
        debug_projection(arg!(ed, 0)).unwrap_or_else(|| Value::array(PhpArray::new()))
    );
}
fn serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let object = arg!(ed, 0).as_object().unwrap();
    let state = object
        .native_object_state::<State>()
        .cloned()
        .unwrap_or_default();
    let mut data = PhpArray::with_packed_capacity(2);
    data.push(Value::array(PhpArray::new()));
    data.push(state.projection());
    ret!(rv, Value::array(data));
}
fn unserialize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let state = (|| {
        let data = arg!(ed, 1).as_array()?;
        if data.len() != 2 || !data.get_int(0)?.as_array()?.is_empty() {
            return None;
        }
        let words = data.get_int(1)?.as_array()?;
        if words.len() != 4 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for index in 0..4 {
            let hex = words.get_int(index)?.as_str()?;
            if hex.len() != 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            bytes[index as usize * 8..index as usize * 8 + 8]
                .copy_from_slice(&u64::from_str_radix(hex, 16).ok()?.to_be_bytes());
        }
        State::bytes(&bytes)
    })();
    if let Some(state) = state {
        *arg!(ed, 0)
            .as_object_mut()
            .unwrap()
            .native_object_state_mut::<State>() = state;
    } else {
        eg.exception = Some(make_error_value(
            "Exception",
            &format!("Invalid serialization data for {XOSHIRO} object"),
        ));
    }
    Ok(())
}

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    use ParamTypeHint::{Array, Int, Nullable, String, Union, Void};
    eg.register_internal_method_contract(
        ENGINE,
        "generate",
        false,
        0,
        &[],
        vec![],
        String,
        &[],
        false,
    );
    let mut engine = empty_internal_type(ENGINE, vec![], true, false);
    engine.abstract_methods.push("generate".into());
    eg.register_class(engine).unwrap();
    eg.register_class(empty_internal_type(
        CRYPTO,
        vec![ENGINE.into()],
        true,
        false,
    ))
    .unwrap();
    let mut exception = empty_internal_type("Random\\RandomException", vec![], false, false);
    exception.parent = Some("Exception".into());
    eg.register_class_with_complete_native_parent(exception)
        .unwrap();
    let rows: &[(
        &str,
        &str,
        InternalFunctionHandler,
        &[&str],
        Vec<ParamTypeHint>,
        ParamTypeHint,
    )] = &[
        (SECURE, "generate", secure, &[], vec![], String),
        (
            XOSHIRO,
            "__construct",
            construct,
            &["seed"],
            vec![Union(vec![
                String,
                Int,
                Nullable(Box::new(ParamTypeHint::None)),
            ])],
            ParamTypeHint::None,
        ),
        (XOSHIRO, "generate", generate, &[], vec![], String),
        (XOSHIRO, "jump", jump, &[], vec![], Void),
        (XOSHIRO, "jumpLong", jump_long, &[], vec![], Void),
        (XOSHIRO, "__serialize", serialize, &[], vec![], Array),
        (
            XOSHIRO,
            "__unserialize",
            unserialize,
            &["data"],
            vec![Array],
            Void,
        ),
        (XOSHIRO, "__debugInfo", debug_info, &[], vec![], Array),
    ];
    let mut functions = Vec::new();
    for (owner, name, handler, names, hints, result) in rows {
        let defaults = if *name == "__construct" {
            vec![Some("null")]
        } else {
            vec![None; names.len()]
        };
        let required = defaults.iter().filter(|v| v.is_none()).count() as u32;
        eg.register_internal_method_contract(
            owner,
            name,
            false,
            required,
            names,
            hints.clone(),
            result.clone(),
            &defaults,
            false,
        );
        let mut function = Box::new(make_internal_method(
            *handler,
            names.len() as u32 + 1,
            required,
            names.iter().map(|n| (*n).into()).collect(),
        ));
        function.common.sig.param_type_hints = hints.clone();
        function.common.plan.call = crate::vm::function::CallStrategy::Full;
        let pointer = &function.common as *const FunctionCommon;
        eg.function_table
            .insert(internal_method_lookup_name(owner, name), pointer);
        eg.bind_latest_internal_method_body(owner, name, pointer);
        eg.method_declaring_class.insert(pointer, (*owner).into());
        eg.register_internal_function_display_name(
            pointer,
            internal_method_display_name(owner, name),
        );
        eg.register_internal_function_reflection_metadata(
            pointer,
            defaults.iter().map(|v| v.map(|_| Value::null())).collect(),
            "random",
        );
        functions.push(function);
    }
    eg.register_class(empty_internal_type(
        SECURE,
        vec![CRYPTO.into()],
        false,
        true,
    ))
    .unwrap();
    eg.register_class(empty_internal_type(
        XOSHIRO,
        vec![ENGINE.into()],
        false,
        true,
    ))
    .unwrap();
    functions
}
