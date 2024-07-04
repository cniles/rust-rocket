pub fn update_average(prev_estimate: f64, measurement: f64, n: u32) -> f64 {
    prev_estimate + (measurement - prev_estimate) / (n as f64)
}
