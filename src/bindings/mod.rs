#[no_mangle]
pub unsafe extern "C" fn give_me_five() -> u64 {
    println!("High five!");
    5
}