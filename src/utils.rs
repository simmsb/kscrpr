use color_eyre::Report;

pub fn fuck_error(report: &Report) -> &(dyn std::error::Error + 'static) {
    report.as_ref()
}
