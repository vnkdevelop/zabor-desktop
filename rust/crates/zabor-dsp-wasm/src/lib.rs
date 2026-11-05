use std::cell::RefCell;
use zabor_dsp::measure_voicing_ring;

struct State {
    input: [f32; 512],
    scratch: [f32; 512],
    result: [f64; 2],
}

thread_local! {
    static STATE: RefCell<Option<Box<State>>> = const { RefCell::new(None) };
}

#[allow(unsafe_code)]
#[unsafe(export_name = "zabor_voicing_abi")]
pub extern "C" fn abi() -> u32 {
    1
}

#[allow(unsafe_code)]
#[unsafe(export_name = "zabor_voicing_create")]
pub extern "C" fn create() -> u32 {
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        if state.is_none() {
            *state = Some(Box::new(State {
                input: [0.0; 512],
                scratch: [0.0; 512],
                result: [0.0; 2],
            }));
        }
        1
    })
}

#[allow(unsafe_code)]
#[unsafe(export_name = "zabor_voicing_input")]
pub extern "C" fn input_pointer() -> usize {
    STATE.with(|cell| {
        cell.borrow()
            .as_ref()
            .map_or(0, |state| state.input.as_ptr() as usize)
    })
}

#[allow(unsafe_code)]
#[unsafe(export_name = "zabor_voicing_result")]
pub extern "C" fn result_pointer() -> usize {
    STATE.with(|cell| {
        cell.borrow()
            .as_ref()
            .map_or(0, |state| state.result.as_ptr() as usize)
    })
}

#[allow(unsafe_code)]
#[unsafe(export_name = "zabor_voicing_process")]
pub extern "C" fn process(write_index: u32, filled: u32) -> u32 {
    if write_index >= 512 || filled > 512 {
        return 0;
    }
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let Some(state) = state.as_mut() else {
            return 0;
        };
        let result = measure_voicing_ring(
            &state.input,
            &mut state.scratch,
            write_index as usize,
            filled as usize,
        );
        state.result = [result.score, result.lag as f64];
        1
    })
}

#[allow(unsafe_code)]
#[unsafe(export_name = "zabor_voicing_dispose")]
pub extern "C" fn dispose() {
    STATE.with(|cell| {
        cell.borrow_mut().take();
    });
}
