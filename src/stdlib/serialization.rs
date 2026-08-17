use crate::runtime::ExecutorGlobals;
use std::collections::HashMap;

use crate::value::{ArrayKey, PhpArray, PhpObject, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

fn argument(ed: *mut ExecuteData, index: u32) -> Value {
    crate::stdlib::owned_argument(ed, index)
}

fn return_value(rv: *mut Value, value: Value) -> Result<(), VmError> {
    crate::stdlib::write_return_value(rv, value);
    Ok(())
}

struct SerializeState {
    next_reference: usize,
    objects: HashMap<usize, usize>,
    references: HashMap<usize, usize>,
}

impl SerializeState {
    fn new() -> Self {
        Self {
            next_reference: 1,
            objects: HashMap::new(),
            references: HashMap::new(),
        }
    }
}

fn serialize_value(
    value: &Value,
    output: &mut String,
    eg: &mut ExecutorGlobals,
    state: &mut SerializeState,
) -> Result<(), VmError> {
    let reference = state.next_reference;
    state.next_reference += 1;
    if let Some(identity) = value.reference_identity() {
        if let Some(reference) = state.references.get(&identity) {
            output.push_str("R:");
            output.push_str(&reference.to_string());
            output.push(';');
            return Ok(());
        }
        state.references.insert(identity, reference);
    }
    let value = value.dereferenced();
    match value.value_type() {
        ValueType::Undef | ValueType::Null => output.push_str("N;"),
        ValueType::False => output.push_str("b:0;"),
        ValueType::True => output.push_str("b:1;"),
        ValueType::Long => {
            output.push_str("i:");
            output.push_str(&value.as_long().unwrap().to_string());
            output.push(';');
        }
        ValueType::Double => {
            let number = value.as_double().unwrap();
            output.push_str("d:");
            if number.is_nan() {
                output.push_str("NAN");
            } else if number == f64::INFINITY {
                output.push_str("INF");
            } else if number == f64::NEG_INFINITY {
                output.push_str("-INF");
            } else {
                output.push_str(&number.to_string());
            }
            output.push(';');
        }
        ValueType::String => {
            let string = value.as_str().unwrap();
            output.push_str("s:");
            output.push_str(&string.len().to_string());
            output.push_str(":\"");
            output.push_str(string);
            output.push_str("\";");
        }
        ValueType::Array => {
            let array = value.as_array().unwrap();
            output.push_str("a:");
            output.push_str(&array.len().to_string());
            output.push_str(":{");
            for (key, member) in array.iter() {
                match key {
                    ArrayKey::Int(key) => {
                        output.push_str("i:");
                        output.push_str(&key.to_string());
                        output.push(';');
                    }
                    ArrayKey::String(key) => {
                        output.push_str("s:");
                        output.push_str(&key.len().to_string());
                        output.push_str(":\"");
                        output.push_str(&key);
                        output.push_str("\";");
                    }
                }
                serialize_value(member, output, eg, state)?;
            }
            output.push('}');
        }
        ValueType::Object => {
            let identity = value
                .object_identity()
                .expect("object value lost its identity");
            if let Some(reference) = state.objects.get(&identity) {
                output.push_str("r:");
                output.push_str(&reference.to_string());
                output.push(';');
                return Ok(());
            }
            state.objects.insert(identity, reference);

            let object = value.as_object().expect("object value lost its payload");
            let class_name = object.class_name.to_string();
            drop(object);
            if class_name.eq_ignore_ascii_case("Generator") {
                eg.exception = Some(crate::value::make_error_value(
                    "Exception",
                    "Serialization of 'Generator' is not allowed",
                ));
                return Ok(());
            }
            let properties =
                match crate::stdlib::call_object_public_method(eg, value, "__serialize", &[])? {
                    Some(serialized) => {
                        if eg.exception.is_some() {
                            return Ok(());
                        }
                        let Some(properties) = serialized.as_array().cloned() else {
                            eg.exception = Some(crate::value::make_error_value(
                                "TypeError",
                                &format!("{class_name}::__serialize() must return an array"),
                            ));
                            return Ok(());
                        };
                        properties
                    }
                    None => {
                        let mut properties = PhpArray::new();
                        if let Some(object) = value.as_object() {
                            object.for_each_property(|name, member| {
                                if member.value_type() != ValueType::Undef {
                                    properties.set_str(name, member.clone());
                                }
                            });
                        }
                        properties
                    }
                };
            if eg.exception.is_some() {
                return Ok(());
            }
            output.push_str("O:");
            output.push_str(&class_name.len().to_string());
            output.push_str(":\"");
            output.push_str(&class_name);
            output.push_str("\":");
            output.push_str(&properties.len().to_string());
            output.push_str(":{");
            for (key, member) in properties.iter() {
                match key {
                    ArrayKey::Int(key) => {
                        output.push_str("i:");
                        output.push_str(&key.to_string());
                        output.push(';');
                    }
                    ArrayKey::String(key) => {
                        output.push_str("s:");
                        output.push_str(&key.len().to_string());
                        output.push_str(":\"");
                        output.push_str(&key);
                        output.push_str("\";");
                    }
                }
                serialize_value(member, output, eg, state)?;
            }
            output.push('}');
        }
        _ => {
            eg.exception = Some(crate::value::make_error_value(
                "Error",
                "Serialization of this value type is not supported",
            ));
        }
    }
    Ok(())
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
    next_reference: usize,
    references: HashMap<usize, Value>,
}

enum AllowedClasses {
    All,
    None,
    List(Vec<String>),
}

impl AllowedClasses {
    fn allows(&self, class_name: &str) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::List(classes) => classes
                .iter()
                .any(|class| class.eq_ignore_ascii_case(class_name)),
        }
    }
}

fn allocate_object(eg: &mut ExecutorGlobals, class_name: &str) -> Result<Value, ()> {
    if eg.find_class(class_name).is_none() {
        crate::stdlib::autoload::ensure_symbol_loaded(eg, class_name).map_err(|_| ())?;
    }
    let object = eg.find_class(class_name).map_or_else(
        || PhpObject::dynamic(class_name.to_string(), 0, HashMap::new()),
        |class| {
            if class.class_id == 0 {
                PhpObject::dynamic(class.name.clone(), 0, HashMap::new())
            } else {
                PhpObject::with_layout(
                    class.class_id,
                    class.property_layout.clone(),
                    class.property_defaults.as_ref().to_vec(),
                )
            }
        },
    );
    Ok(Value::object(object))
}

fn incomplete_object(class_name: &str, properties: &PhpArray) -> Value {
    let mut values = HashMap::new();
    values.insert(
        "__PHP_Incomplete_Class_Name".to_string(),
        Value::string(class_name),
    );
    for (key, value) in properties.iter() {
        let name = match key {
            ArrayKey::Int(key) => key.to_string(),
            ArrayKey::String(key) => key.clone(),
        };
        values.insert(name, value.clone());
    }
    Value::object(PhpObject::dynamic(
        "__PHP_Incomplete_Class".to_string(),
        0,
        values,
    ))
}

fn populate_object_properties(
    eg: &ExecutorGlobals,
    object: &Value,
    class_name: &str,
    properties: &PhpArray,
) {
    let Some(mut object) = object.as_object_mut() else {
        return;
    };
    for (key, value) in properties.iter() {
        let ArrayKey::String(key) = key else {
            continue;
        };
        let plain_name = key
            .strip_prefix('\0')
            .and_then(|key| key.split_once('\0').map(|(_, name)| name))
            .unwrap_or(key.as_str());
        let storage_key =
            crate::runtime::resolve_property_key(eg, class_name, plain_name, Some(class_name));
        object.set_property(&storage_key, value.clone());
    }
}

impl<'a> Parser<'a> {
    fn byte(&mut self) -> Result<u8, ()> {
        let byte = *self.input.get(self.position).ok_or(())?;
        self.position += 1;
        Ok(byte)
    }

    fn expect(&mut self, expected: u8) -> Result<(), ()> {
        (self.byte()? == expected).then_some(()).ok_or(())
    }

    fn token(&mut self, delimiter: u8) -> Result<&'a [u8], ()> {
        let start = self.position;
        let relative = self.input.get(start..).ok_or(())?;
        let length = relative
            .iter()
            .position(|byte| *byte == delimiter)
            .ok_or(())?;
        self.position = start + length + 1;
        Ok(&self.input[start..start + length])
    }

    fn integer(&mut self, delimiter: u8) -> Result<i64, ()> {
        std::str::from_utf8(self.token(delimiter)?)
            .map_err(|_| ())?
            .parse()
            .map_err(|_| ())
    }

    fn key(&mut self) -> Result<ArrayKey, ()> {
        match self.byte()? {
            b'i' => {
                self.expect(b':')?;
                Ok(ArrayKey::Int(self.integer(b';')?))
            }
            b's' => {
                self.expect(b':')?;
                let length = usize::try_from(self.integer(b':')?).map_err(|_| ())?;
                self.expect(b'"')?;
                let end = self.position.checked_add(length).ok_or(())?;
                let bytes = self.input.get(self.position..end).ok_or(())?;
                self.position = end;
                self.expect(b'"')?;
                self.expect(b';')?;
                Ok(ArrayKey::String(
                    std::str::from_utf8(bytes).map_err(|_| ())?.to_string(),
                ))
            }
            _ => Err(()),
        }
    }

    fn value(
        &mut self,
        eg: &mut ExecutorGlobals,
        allowed_classes: &AllowedClasses,
    ) -> Result<Value, ()> {
        let reference = self.next_reference;
        self.next_reference += 1;
        let value = match self.byte()? {
            b'N' => {
                self.expect(b';')?;
                Ok(Value::null())
            }
            b'b' => {
                self.expect(b':')?;
                let value = self.integer(b';')?;
                match value {
                    0 => Ok(Value::bool(false)),
                    1 => Ok(Value::bool(true)),
                    _ => Err(()),
                }
            }
            b'i' => {
                self.expect(b':')?;
                Ok(Value::long(self.integer(b';')?))
            }
            b'd' => {
                self.expect(b':')?;
                let token = std::str::from_utf8(self.token(b';')?).map_err(|_| ())?;
                let number = match token {
                    "NAN" => f64::NAN,
                    "INF" => f64::INFINITY,
                    "-INF" => f64::NEG_INFINITY,
                    value => value.parse().map_err(|_| ())?,
                };
                Ok(Value::double(number))
            }
            b's' => {
                self.expect(b':')?;
                let length = self.integer(b':')?;
                let length = usize::try_from(length).map_err(|_| ())?;
                self.expect(b'"')?;
                let end = self.position.checked_add(length).ok_or(())?;
                let bytes = self.input.get(self.position..end).ok_or(())?;
                self.position = end;
                self.expect(b'"')?;
                self.expect(b';')?;
                let string = std::str::from_utf8(bytes).map_err(|_| ())?;
                Ok(Value::string(string))
            }
            b'a' => {
                self.expect(b':')?;
                let length = self.integer(b':')?;
                let length = usize::try_from(length).map_err(|_| ())?;
                self.expect(b'{')?;
                let mut array = PhpArray::with_hash_capacity(length);
                for _ in 0..length {
                    let key = self.key()?;
                    let member = self.value(eg, allowed_classes)?;
                    match key {
                        ArrayKey::Int(key) => array.set_int(key, member),
                        ArrayKey::String(key) => array.set_str(&key, member),
                    }
                }
                self.expect(b'}')?;
                Ok(Value::array(array))
            }
            b'O' => {
                self.expect(b':')?;
                let class_length = self.integer(b':')?;
                let class_length = usize::try_from(class_length).map_err(|_| ())?;
                self.expect(b'"')?;
                let class_end = self.position.checked_add(class_length).ok_or(())?;
                let class_bytes = self.input.get(self.position..class_end).ok_or(())?;
                self.position = class_end;
                self.expect(b'"')?;
                self.expect(b':')?;
                let property_count = self.integer(b':')?;
                let property_count = usize::try_from(property_count).map_err(|_| ())?;
                self.expect(b'{')?;
                let class_name = std::str::from_utf8(class_bytes).map_err(|_| ())?;
                let allowed = allowed_classes.allows(class_name);
                if allowed && class_name.eq_ignore_ascii_case("Generator") {
                    eg.exception = Some(crate::value::make_error_value(
                        "Exception",
                        "Unserialization of 'Generator' is not allowed",
                    ));
                    return Err(());
                }
                let object = if allowed {
                    allocate_object(eg, class_name)?
                } else {
                    incomplete_object(class_name, &PhpArray::new())
                };
                // Publish the object before parsing properties so `r:N;` can
                // close self-references and longer object cycles.
                self.references.insert(reference, object.clone());
                let mut properties = PhpArray::with_hash_capacity(property_count);
                for _ in 0..property_count {
                    let key = self.key()?;
                    let member = self.value(eg, allowed_classes)?;
                    match key {
                        ArrayKey::Int(key) => properties.set_int(key, member),
                        ArrayKey::String(key) => properties.set_str(&key, member),
                    }
                }
                self.expect(b'}')?;
                if !allowed {
                    let object = incomplete_object(class_name, &properties);
                    self.references.insert(reference, object.clone());
                    return Ok(object);
                }

                let serialized = Value::array(properties.clone());
                match crate::stdlib::call_object_public_method(
                    eg,
                    &object,
                    "__unserialize",
                    std::slice::from_ref(&serialized),
                )
                .map_err(|_| ())?
                {
                    Some(_) => {}
                    None => populate_object_properties(eg, &object, class_name, &properties),
                }
                Ok(object)
            }
            b'C' => {
                self.expect(b':')?;
                let class_length = usize::try_from(self.integer(b':')?).map_err(|_| ())?;
                self.expect(b'"')?;
                let class_end = self.position.checked_add(class_length).ok_or(())?;
                let class_bytes = self.input.get(self.position..class_end).ok_or(())?;
                self.position = class_end;
                self.expect(b'"')?;
                let class_name = std::str::from_utf8(class_bytes).map_err(|_| ())?;
                if allowed_classes.allows(class_name)
                    && class_name.eq_ignore_ascii_case("Generator")
                {
                    eg.exception = Some(crate::value::make_error_value(
                        "Exception",
                        "Unserialization of 'Generator' is not allowed",
                    ));
                }
                Err(())
            }
            b'r' | b'R' => {
                self.expect(b':')?;
                let target = usize::try_from(self.integer(b';')?).map_err(|_| ())?;
                self.references.get(&target).cloned().ok_or(())
            }
            _ => Err(()),
        }?;
        self.references
            .entry(reference)
            .or_insert_with(|| value.clone());
        Ok(value)
    }
}

pub(super) fn serialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut output = String::new();
    let mut state = SerializeState::new();
    serialize_value(&argument(ed, 0), &mut output, eg, &mut state)?;
    if eg.exception.is_some() {
        return return_value(rv, Value::null());
    }
    return_value(rv, Value::string(output))
}

pub(super) fn unserialize(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let input = argument(ed, 0);
    let Some(input) = input.as_str() else {
        return return_value(rv, Value::bool(false));
    };
    let options = argument(ed, 1);
    let allowed_classes = options
        .as_array()
        .and_then(|options| options.get_str("allowed_classes"))
        .map_or(AllowedClasses::All, |allowed| match allowed.value_type() {
            ValueType::False => AllowedClasses::None,
            ValueType::Array => AllowedClasses::List(
                allowed
                    .as_array()
                    .unwrap()
                    .values()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            ),
            _ => AllowedClasses::All,
        });
    let mut parser = Parser {
        input: input.as_bytes(),
        position: 0,
        next_reference: 1,
        references: HashMap::new(),
    };
    match parser.value(eg, &allowed_classes) {
        Ok(value) if parser.position == parser.input.len() => return_value(rv, value),
        _ => return_value(rv, Value::bool(false)),
    }
}
