#[cfg(all(feature = "jit-prototype", target_arch = "x86_64", target_os = "linux"))]
fn main() {
    use rphp::jit::{
        CompiledX86StraightLongLoop, NATIVE_STRAIGHT_LONG_MAX_OPERATIONS,
        NativeStraightLongLoopConfig, NativeStraightLongOperation,
    };
    use rphp::vm::function::ScalarLongOpKind;
    use rphp::vm::quick::QuickLongOperand;
    use std::hint::black_box;
    use std::time::Instant;

    let iterations = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(10_000_000);
    let sample_count = std::env::args()
        .nth(2)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(51);
    assert!(iterations > 0);
    assert!(sample_count > 0);

    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(0),
        result: 2,
        destination: 1,
    };
    let config = NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(iterations),
        operations,
        operation_count: 1,
        post_result: None,
    };

    let mut compile_times = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let started = Instant::now();
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        black_box(program.code());
        compile_times.push(started.elapsed().as_secs_f64());
    }

    let program = CompiledX86StraightLongLoop::compile(config).unwrap();
    for _ in 0..3 {
        let mut slots = [0_i64; 64];
        slots[1] = 10;
        program.call(black_box(&mut slots)).unwrap();
        black_box(slots);
    }

    let mut execute_times = Vec::with_capacity(sample_count);
    let mut final_slots = [0_i64; 64];
    for _ in 0..sample_count {
        let mut slots = [0_i64; 64];
        slots[1] = 10;
        let started = Instant::now();
        program.call(black_box(&mut slots)).unwrap();
        execute_times.push(started.elapsed().as_secs_f64());
        final_slots = black_box(slots);
    }

    compile_times.sort_by(f64::total_cmp);
    execute_times.sort_by(f64::total_cmp);
    let percentile = |samples: &[f64], numerator: usize| {
        let index = (samples.len() - 1) * numerator / 10;
        samples[index]
    };
    let compile_median = compile_times[compile_times.len() / 2];
    let execute_median = execute_times[execute_times.len() / 2];
    let expected = 10 + (iterations - 1) * iterations / 2;
    assert_eq!(&final_slots[..3], &[iterations, expected, expected]);

    println!("iterations={iterations} samples={sample_count}");
    println!(
        "result={},{},{}",
        final_slots[0], final_slots[1], final_slots[2]
    );
    println!("compile_median_us={:.3}", compile_median * 1_000_000.0);
    println!(
        "execute_p10_ms={:.6}",
        percentile(&execute_times, 1) * 1_000.0
    );
    println!("execute_median_ms={:.6}", execute_median * 1_000.0);
    println!(
        "execute_p90_ms={:.6}",
        percentile(&execute_times, 9) * 1_000.0
    );
    println!(
        "execute_ns_per_iteration={:.6}",
        execute_median * 1_000_000_000.0 / iterations as f64
    );
}

#[cfg(not(all(feature = "jit-prototype", target_arch = "x86_64", target_os = "linux")))]
fn main() {
    eprintln!("this benchmark requires --features jit-prototype on x86-64 Linux");
    std::process::exit(2);
}
