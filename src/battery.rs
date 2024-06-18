use embedded_hal::digital::InputPin;
use esp_idf_hal::{
    adc::{Adc, AdcChannelDriver, AdcConfig, AdcDriver},
    gpio::ADCPin,
    peripheral::Peripheral,
    sys::EspError,
};

pub struct Battery<C, ADC, V>
where
    ADC: Adc + 'static,
    V: ADCPin,
{
    charge_pin: C,
    adc_driver: AdcDriver<'static, ADC>,
    adc_channel_driver: AdcChannelDriver<'static, 3, V>,
}

#[derive(Debug, Clone, Copy)]
pub enum BatteryError<C, V> {
    ChargeError(C),
    VoltageError(V),
}

impl<C, ADC, V> Battery<C, ADC, V>
where
    C: InputPin,
    ADC: Adc + 'static,
    V: ADCPin<Adc = ADC>,
{
    pub fn new(charge_pin: C, adc: impl Peripheral<P = ADC> + 'static, adc_pin: V) -> Self {
        let config = AdcConfig::new().calibration(true);
        let adc_driver = AdcDriver::new(adc, &config).unwrap();
        let adc_channel_driver = AdcChannelDriver::new(adc_pin).unwrap();
        Self {
            charge_pin,
            adc_driver,
            adc_channel_driver,
        }
    }

    pub fn charging(&mut self) -> Result<bool, BatteryError<C::Error, EspError>> {
        let mut charging = 0;
        for _i in 0..10 {
            if self
                .charge_pin
                .is_high()
                .map_err(BatteryError::ChargeError)?
            {
                charging = charging + 1;
            }
        }

        Ok(charging == 0)
    }

    pub fn voltage(&mut self) -> Result<f64, BatteryError<C::Error, EspError>> {
        let v = self
            .adc_driver
            .read(&mut self.adc_channel_driver)
            .map_err(BatteryError::VoltageError)?;
        Ok((v as f64) / 4095.0 * 3.7)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_foo() {
        assert!(false);
    }
}
