//! Confirmed-song interval in CMusicSelectScene, sampled after its original update.
//! Evidence: docs/iidx34-0100917-decision-analysis.md. No game functions are called here.

pub const MEMBER_RVA: usize = 0x8d9330;
pub const GUARDS: &[(usize, &str)] = &[
    (0x8d8290, "48895c240848896c2410488974241848"),
    (0x8da130, "48895c24084889742410574883ec3033"),
    (0x8d9330, "488bc441564881ec8000000048c740a8"),
    (0x8d93d8, "e8d3faffffe89eefcdffe8a9eccdff48"),
    (0x8d9684, "488d05e5080000488944244044897424"),
    (0x8d8ed7, "49898500210000e81d13d2ff33db3dce"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SampleError {
    #[error("Decision state memory is unreadable")]
    Unreadable,
    #[error("Decision state read has an unexpected size")]
    Truncated,
    #[error("Decision state address overflow")]
    AddressOverflow,
}

/// Only the active, already-executed decision member qualifies. The pending
/// member, a modal, and the shared exit member must never start/extend spinning.
pub fn confirmed_record(
    base: usize,
    scene: usize,
    read: impl Fn(usize, usize) -> Result<Vec<u8>, SampleError>,
) -> Result<Option<usize>, SampleError> {
    let bytes = |address: usize, offset: usize, size: usize| {
        let value = read(
            address
                .checked_add(offset)
                .ok_or(SampleError::AddressOverflow)?,
            size,
        )?;
        if value.len() != size {
            return Err(SampleError::Truncated);
        }
        Ok(value)
    };
    let pointer = |address, offset| -> Result<usize, SampleError> {
        Ok(u64::from_le_bytes(bytes(address, offset, 8)?.try_into().unwrap()) as usize)
    };
    let controller = pointer(scene, 152)?;
    if controller == 0 {
        return Ok(None);
    }
    let member = bytes(controller, 8, 12)?;
    let target = u64::from_le_bytes(member[..8].try_into().unwrap()) as usize;
    let adjustment = i32::from_le_bytes(member[8..].try_into().unwrap());
    if target
        != base
            .checked_add(MEMBER_RVA)
            .ok_or(SampleError::AddressOverflow)?
        || adjustment != 0
    {
        return Ok(None);
    }
    let frames = u32::from_le_bytes(bytes(controller, 88, 4)?.try_into().unwrap());
    if frames == 0 {
        return Ok(None);
    }
    let record = pointer(scene, 8448)?;
    Ok((record != 0).then_some(record))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(member: usize, adjustment: i32, frames: u32, record: usize) -> Option<usize> {
        confirmed_record(0x10000000, 0x1000, |address, size| {
            let value = match (address, size) {
                (0x1098, 8) => 0x2000u64.to_le_bytes().to_vec(),
                (0x2008, 12) => [
                    (member as u64).to_le_bytes().as_slice(),
                    &adjustment.to_le_bytes(),
                ]
                .concat(),
                (0x2058, 4) => frames.to_le_bytes().to_vec(),
                (0x3100, 8) => (record as u64).to_le_bytes().to_vec(),
                _ => panic!("Unexpected read {address:x}+{size}"),
            };
            Ok(value)
        })
        .unwrap()
    }

    #[test]
    fn only_executed_decision_member_reports_the_committed_record() {
        assert_eq!(sample(0x10000000 + MEMBER_RVA, 0, 1, 0x4000), Some(0x4000));
        assert_eq!(
            sample(0x10000000 + MEMBER_RVA, 0, 500, 0x5000),
            Some(0x5000)
        );
        assert_eq!(sample(0x10000000 + MEMBER_RVA, 0, 0, 0x4000), None);
        assert_eq!(sample(0x10000000 + MEMBER_RVA, 4, 1, 0x4000), None);
        assert_eq!(sample(0x10000000 + MEMBER_RVA, 0, 1, 0), None);
        // Interactive selection, confirmation dialog, and the shared scene exit.
        for member in [0x8da0a0, 0x8d90c0, 0x8d9f70] {
            assert_eq!(sample(0x10000000 + member, 0, 1, 0x4000), None);
        }
    }

    #[test]
    fn missing_or_unreadable_controller_does_not_reuse_a_previous_decision() {
        assert_eq!(confirmed_record(0, 0, |_, _| Ok(vec![0; 8])), Ok(None));
        assert_eq!(
            confirmed_record(0, 0, |_, _| Err(SampleError::Unreadable)),
            Err(SampleError::Unreadable)
        );
        assert_eq!(
            confirmed_record(0, 0, |_, _| Ok(vec![0])),
            Err(SampleError::Truncated)
        );
        assert_eq!(
            confirmed_record(0, usize::MAX, |_, _| unreachable!()),
            Err(SampleError::AddressOverflow)
        );
    }
}
