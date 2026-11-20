// SPDX-License-Identifier: MIT
use crate::vulkan_fg_abi::rsf_fgvk;
use std::ptr;
use std::sync::{Mutex, MutexGuard};

const OK: i32 = 0;
const INVALID: i32 = -1;
const NOT_READY: i32 = -8;

type Context = *mut rsf_fgvk;

#[derive(PartialEq, Eq)]
enum Admission {
    Open,
    Draining,
    Retiring,
    ShutdownPending,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Create,
    Drain(Context),
    Destroy(Context),
    Shutdown,
}

struct Binding {
    context: Context,
    borrowers: usize,
    admission: Admission,
    operation: Option<Operation>,
}

// The binding holds an opaque identity. It never dereferences it or calls an SDK under the lock.
unsafe impl Send for Binding {}

impl Binding {
    const fn new() -> Self {
        Self {
            context: ptr::null_mut(),
            borrowers: 0,
            admission: Admission::Open,
            operation: None,
        }
    }

    fn attach(&mut self, context: Context) -> i32 {
        if context.is_null() {
            return INVALID;
        }
        if self.admission != Admission::Open || (!self.context.is_null() && self.context != context)
        {
            return NOT_READY;
        }
        self.context = context;
        OK
    }

    fn clear(&mut self) {
        self.context = ptr::null_mut();
        self.admission = Admission::Open;
    }

    fn closing(&self) -> bool {
        self.admission != Admission::Open || self.operation == Some(Operation::Shutdown)
    }

    fn begin(&mut self, operation: Operation) -> Result<(), i32> {
        if self.operation.is_some() {
            return Err(NOT_READY);
        }
        match operation {
            Operation::Drain(context) | Operation::Destroy(context) if context == self.context => {
                if matches!(operation, Operation::Destroy(_)) {
                    self.admission = Admission::Retiring;
                } else if self.admission != Admission::Retiring {
                    self.admission = Admission::Draining;
                }
                if self.borrowers != 0 {
                    return Err(NOT_READY);
                }
            }
            Operation::Shutdown if self.borrowers != 0 => {
                if self.admission == Admission::Open {
                    self.admission = Admission::ShutdownPending;
                }
                return Err(NOT_READY);
            }
            _ => {}
        }
        self.operation = Some(operation);
        Ok(())
    }

    fn finish(&mut self, operation: Operation, success: bool) {
        debug_assert!(self.operation == Some(operation));
        match operation {
            Operation::Drain(context)
                if context == self.context && self.admission == Admission::Draining =>
            {
                self.admission = Admission::Open;
            }
            Operation::Destroy(context) if success && context == self.context => self.clear(),
            Operation::Shutdown if success => self.clear(),
            Operation::Shutdown if self.admission == Admission::ShutdownPending => {
                self.admission = Admission::Open;
            }
            _ => {}
        }
        self.operation = None;
    }
}

static BINDING: Mutex<Binding> = Mutex::new(Binding::new());

fn binding() -> MutexGuard<'static, Binding> {
    BINDING.lock().unwrap()
}

pub(super) struct ProviderOperation {
    operation: Operation,
    success: bool,
}

impl ProviderOperation {
    fn begin(operation: Operation) -> Result<Self, i32> {
        binding().begin(operation)?;
        Ok(Self {
            operation,
            success: false,
        })
    }

    pub(super) fn create() -> Result<Self, i32> {
        Self::begin(Operation::Create)
    }

    pub(super) fn drain(context: Context) -> Result<Self, i32> {
        Self::begin(Operation::Drain(context))
    }

    pub(super) fn destroy(context: Context) -> Result<Self, i32> {
        Self::begin(Operation::Destroy(context))
    }

    pub(super) fn attach_created(&self, context: Context) -> i32 {
        debug_assert!(self.operation == Operation::Create);
        binding().attach(context)
    }

    pub(super) fn destroyed(&mut self) {
        self.success = true;
    }
}

impl Drop for ProviderOperation {
    fn drop(&mut self) {
        binding().finish(self.operation, self.success);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fgvk_presentation_attach(context: Context) -> i32 {
    let mut state = binding();
    if context.is_null() {
        return INVALID;
    }
    if state.operation.is_some() {
        return NOT_READY;
    }
    state.attach(context)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fgvk_presentation_unbind() -> i32 {
    let mut state = binding();
    if state.context.is_null() {
        return OK;
    }
    if state.operation.is_some() {
        return NOT_READY;
    }
    state.admission = Admission::Retiring;
    if state.borrowers != 0 {
        return NOT_READY;
    }
    state.clear();
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fgvk_presentation_borrow(closing: *mut u32) -> Context {
    let mut state = binding();
    let closed = state.closing();
    unsafe { closing.write(u32::from(closed)) };
    if closed || state.context.is_null() {
        return ptr::null_mut();
    }
    state.borrowers += 1;
    state.context
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fgvk_presentation_return() {
    binding().borrowers -= 1;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fgvk_presentation_shutdown_begin() -> i32 {
    match binding().begin(Operation::Shutdown) {
        Ok(()) => OK,
        Err(result) => result,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fgvk_presentation_shutdown_end(success: u32) {
    binding().finish(Operation::Shutdown, success != 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_owner_retirement_preserves_a_successor() {
        let mut state = Binding::new();
        let previous = ptr::without_provenance_mut(1);
        let next = ptr::without_provenance_mut(2);
        assert_eq!(state.attach(next), OK);
        let operation = Operation::Destroy(previous);
        assert_eq!(state.begin(operation), Ok(()));
        state.finish(operation, true);
        assert_eq!(state.context, next);
        assert!(!state.closing());
    }

    #[test]
    fn provider_operations_and_sdk_shutdown_cannot_overlap() {
        let mut state = Binding::new();
        let context = ptr::without_provenance_mut(1);
        assert_eq!(state.attach(context), OK);

        let drain = Operation::Drain(context);
        state.borrowers = 1;
        assert_eq!(state.begin(drain), Err(NOT_READY));
        assert!(state.closing());
        state.borrowers = 0;
        assert_eq!(state.begin(drain), Ok(()));
        assert_eq!(state.begin(Operation::Shutdown), Err(NOT_READY));
        state.finish(drain, false);
        assert!(!state.closing());

        state.borrowers = 1;
        assert_eq!(state.begin(Operation::Shutdown), Err(NOT_READY));
        assert!(state.closing());
        state.borrowers = 0;
        assert_eq!(state.begin(Operation::Shutdown), Ok(()));
        let destroy = Operation::Destroy(context);
        assert_eq!(state.begin(destroy), Err(NOT_READY));
        assert_eq!(state.begin(Operation::Create), Err(NOT_READY));
        state.finish(Operation::Shutdown, false);
        assert_eq!(state.context, context);
        assert!(!state.closing());

        assert_eq!(state.begin(destroy), Ok(()));
        assert_eq!(state.begin(Operation::Shutdown), Err(NOT_READY));
        state.finish(destroy, false);
        assert!(state.closing());
        assert_eq!(state.begin(destroy), Ok(()));
        state.finish(destroy, true);
        assert!(state.context.is_null() && !state.closing());
    }
}
