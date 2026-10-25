use super::*;

#[test]
fn deadzone_percentages_use_the_declared_range_and_round_the_neutral_center() {
    let mut signed = phase(unary(
        Operation::Deadzone {
            center_percent: 50.0,
            deadzone_percent: 20.0,
            rounding: Rounding::None,
        },
        number_contract(-10.0, 10.0, None),
        number_contract(-10.0, 10.0, None),
    ));
    feed(&mut signed, 1, 1, sample(6.0));
    assert!((scalar_output(&signed) - 5.0).abs() < 1e-9);

    let mut unipolar = phase(unary(
        Operation::Deadzone {
            center_percent: 25.0,
            deadzone_percent: 20.0,
            rounding: Rounding::None,
        },
        number_contract(0.0, 100.0, None),
        number_contract(0.0, 100.0, None),
    ));
    feed(&mut unipolar, 1, 1, sample(30.0));
    assert_eq!(scalar_output(&unipolar), 25.0);
    feed(&mut unipolar, 2, 2, sample(100.0));
    assert_eq!(scalar_output(&unipolar), 100.0);

    let mut rounded_center = phase(unary(
        Operation::Deadzone {
            center_percent: 45.0,
            deadzone_percent: 20.0,
            rounding: Rounding::Nearest,
        },
        integer_contract(0, 10),
        integer_contract(0, 10),
    ));
    feed(&mut rounded_center, 1, 1, sample(5.0));
    assert_eq!(scalar_output(&rounded_center), 5.0);
}
