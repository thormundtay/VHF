#[cfg(feature = "o3")]
use vhf_parse::{ParseResult, VHFparse};

#[cfg(feature = "o3")]
fn main() -> ParseResult<()> {
    use jiff::Span;
    use pyo3::prepare_freethreaded_python;
    use std::path::PathBuf;
    use vhf_parse::py_binds::{RelTime, StartTime};
    use vhf_parse::v1_python::VHFparser;

    prepare_freethreaded_python();
    let file = PathBuf::from(
        "/dev/shm/2023-07-06T14:04:22.913767_s499_q268435456_F0_laser2_3674mA_20km.bin",
    );
    assert!(file.exists());

    let mut v1p = VHFparser::new(&file, true).unwrap();
    v1p.resolve_m_overflow_idxs()?;
    let start = Span::new().seconds(60);
    let duration = Span::new().seconds(90);

    v1p.update_plot_timing(StartTime::Rel(RelTime(start)), RelTime(duration), false)?;

    Ok(())
}

#[cfg(not(feature = "o3"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Nothig to rest without o3");
    panic!();
}
