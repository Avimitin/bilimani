use std::sync::{
    RwLock,
    atomic::{AtomicI32, Ordering},
};
pub type GetButton = unsafe extern "C" fn(u32, *mut bool, *mut f32) -> i32;
static GET_BUTTON: RwLock<Option<GetButton>> = RwLock::new(None);
static STATUS: AtomicI32 = AtomicI32::new(-1);

pub fn set(get: Option<GetButton>) {
    *GET_BUTTON.write().unwrap() = get;
    STATUS.store(if get.is_some() { -2 } else { -1 }, Ordering::Relaxed);
}
pub fn status() -> i32 {
    STATUS.load(Ordering::Relaxed)
}
pub fn button(button: u32) -> Option<bool> {
    // Keep the read guard through the call; shutdown clears it before returning.
    let api = GET_BUTTON.read().unwrap();
    let get = api.as_ref()?;
    let mut pressed = false;
    let result = unsafe { get(button, &mut pressed, std::ptr::null_mut()) };
    STATUS.store(result, Ordering::Relaxed);
    (result == 0).then_some(pressed)
}
