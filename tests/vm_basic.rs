use rphp::compiler::{
    OpArray, make_internal_function, make_user_function, make_user_function_with_args,
};
use rphp::runtime::ExecutorGlobals;
use rphp::value::Value;
use rphp::vm::execute;
use rphp::vm::frame::ExecuteData;
use rphp::vm::function::FunctionCommon;
use rphp::vm::instruction::{Instruction, OpType};
use rphp::vm::opcode::OpCode;

fn make_eg_with_capture() -> (ExecutorGlobals, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let buf_clone = buf.clone();
    let writer = WriterCapture(buf_clone);
    let eg = ExecutorGlobals::with_output(Box::new(writer));
    (eg, buf)
}

struct WriterCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for WriterCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn captured_output(buf: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buf.lock().unwrap().clone()).unwrap()
}

include!("vm_basic/basic_and_variables.rs");
include!("vm_basic/function_calls.rs");
include!("vm_basic/recursion_interrupts_and_results.rs");
