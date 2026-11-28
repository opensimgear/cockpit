use crate::{Globals, Physical};
use num_bigint::{BigInt, Sign};

pub(super) fn scale(value: &BigInt, globals: &Globals) -> Option<Physical> {
    let (low, high) = (globals.logical_min?, globals.logical_max?);
    let (physical_low, physical_high) = (globals.physical_min?, globals.physical_max?);
    if low >= high || (physical_low == 0 && physical_high == 0) {
        return None;
    }
    let mut denominator = BigInt::from(high) - BigInt::from(low);
    let mut numerator = (value - low) * (BigInt::from(physical_high) - physical_low)
        + BigInt::from(physical_low) * &denominator;
    let mut a = if numerator.sign() == Sign::Minus {
        -numerator.clone()
    } else {
        numerator.clone()
    };
    let mut b = denominator.clone();
    while b != BigInt::from(0) {
        let remainder = a % &b;
        a = b;
        b = remainder;
    }
    numerator /= &a;
    denominator /= a;
    Some(Physical {
        numerator: numerator.to_string(),
        denominator: denominator.to_string(),
        unit: globals.unit.unwrap_or(0),
        exponent: globals.exponent.unwrap_or(0),
    })
}
