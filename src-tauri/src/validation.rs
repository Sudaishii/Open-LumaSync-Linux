pub fn sections(value: [u16; 3]) -> Result<u16, String> {
    let total: u32 = value.iter().map(|&n| n as u32).sum();
    if !(1..=254).contains(&total) {
        return Err("LED layout must contain between 1 and 254 LEDs.".into());
    }
    Ok(total as u16)
}
pub fn finite_range(value: f64, min: f64, max: f64, label: &str) -> Result<f64, String> {
    if !value.is_finite() || value < min || value > max { return Err(format!("{} must be between {} and {}.",label,min,max)); }
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn layout_boundaries() {
        assert_eq!(sections([15,41,15]),Ok(71));
        assert_eq!(sections([0,254,0]),Ok(254));
        assert!(sections([0,0,0]).is_err());
        assert!(sections([254,1,0]).is_err());
        assert!(sections([u16::MAX,u16::MAX,u16::MAX]).is_err());
    }
    #[test] fn numeric_settings_reject_nan_and_out_of_range() {
        assert!(finite_range(f64::NAN,1.,10.,"Speed").is_err());
        assert!(finite_range(f64::INFINITY,1.,10.,"Speed").is_err());
        assert!(finite_range(0.,1.,10.,"Speed").is_err());
        assert_eq!(finite_range(10.,1.,10.,"Speed"),Ok(10.));
    }
}
