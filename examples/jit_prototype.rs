#[cfg(all(
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use rphp::jit::CompiledScalarLongProgram;
    use rphp::vm::function::{
        ScalarLongFunctionPlan, ScalarLongOp, ScalarLongOpKind, ScalarLongProgram, ScalarLongSource,
    };

    let plan = ScalarLongFunctionPlan {
        public_args: 3,
        program: ScalarLongProgram {
            operations: vec![
                ScalarLongOp {
                    kind: ScalarLongOpKind::Add,
                    lhs: ScalarLongSource::Input(0),
                    rhs: ScalarLongSource::Input(1),
                },
                ScalarLongOp {
                    kind: ScalarLongOpKind::Multiply,
                    lhs: ScalarLongSource::Temporary(0),
                    rhs: ScalarLongSource::Input(2),
                },
            ]
            .into_boxed_slice(),
            outputs: [ScalarLongSource::Temporary(1)],
            output_count: 1,
        },
        select: None,
    };
    let function = CompiledScalarLongProgram::compile(&plan)?;
    let first = 7;
    let second = 5;
    let multiplier = 3;
    let result = function.call(&[first, second, multiplier])?;
    let overflow = function.call(&[i64::MAX, 1, multiplier])?;

    println!("generated {} native bytes", function.code().len());
    println!("({first} + {second}) * {multiplier} = {result:?}");
    println!("overflow result = {overflow:?}");
    Ok(())
}

#[cfg(not(all(
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
)))]
fn main() {
    eprintln!("run this prototype on macOS ARM64 with --features jit-prototype");
}
