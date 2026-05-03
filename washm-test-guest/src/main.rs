use washm_test_guest::init;

init!();

#[no_mangle]
pub fn do_math(a: i32, b: i32) -> i32 {
    let mut sum = 0;
    for i in 0..a {
        if i % 2 == 0 {
            sum += b;
        } else {
            sum -= b;
        }
    }
    sum
}

#[test]
fn test_do_math() {
    println!("Marker");
    assert_eq!(do_math(4, 5), 0);
}

fn main() {
    println!("Hello from guest!");
}
