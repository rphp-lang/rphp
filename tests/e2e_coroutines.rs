#![cfg(feature = "coroutines")]

mod common;

#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::net::{SocketAddr, TcpListener, TcpStream};
#[cfg(unix)]
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::runtime::coroutine;
use rphp::stdlib;
use rphp::vm::execute;
use rphp::vm::function::FunctionCommon;

fn run(source: &str) -> Result<String, execute::VmError> {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let main_function = make_user_function(compiled.main);
    let (mut eg, output) = common::make_eg_with_capture();
    let _stdlib = stdlib::register_stdlib(&mut eg);
    let _coroutines = coroutine::register_api(&mut eg);
    for (name, function) in &compiled.functions {
        eg.register_function(name, &function.common as *const FunctionCommon)
            .unwrap();
    }
    for class in compiled.class_defs {
        eg.register_class(class).unwrap();
    }

    execute::execute(&mut eg, &main_function)?;
    let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    Ok(output)
}

#[cfg(unix)]
fn reserve_loopback_address() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

#[cfg(unix)]
fn spawn_loopback_client(address: SocketAddr) -> thread::JoinHandle<[u8; 4]> {
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stream = loop {
            match TcpStream::connect(address) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("failed to connect loopback coroutine client: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(b"ping").unwrap();
        let mut response = [0; 4];
        stream.read_exact(&mut response).unwrap();
        response
    })
}

include!("e2e_coroutines/core.rs");
include!("e2e_coroutines/io.rs");
include!("e2e_coroutines/benchmarks.rs");
