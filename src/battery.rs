use std::rc::Rc;

use embedded_hal::digital::InputPin;
use esp_idf_hal::{
    adc::{
        config::Resolution,
        oneshot::{config::AdcChannelConfig, AdcChannelDriver, AdcDriver},
        AdcConfig,
    },
    gpio::ADCPin,
    peripheral::Peripheral,
    sys::EspError,
};

pub struct Battery<C, T>
where
    T: ADCPin,
{
    charge_pin: C,
    adc_driver: Rc<AdcDriver<'static, T::Adc>>,
    adc_channel_driver: AdcChannelDriver<'static, T, Rc<AdcDriver<'static, T::Adc>>>,
}

#[derive(Debug, Clone, Copy)]
pub enum BatteryError<C, V> {
    ChargeError(C),
    VoltageError(V),
}

impl<C, T> Battery<C, T>
where
    C: InputPin,
    T: ADCPin,
{
    pub fn new(charge_pin: C, adc: impl Peripheral<P = T::Adc> + 'static, adc_pin: T) -> Self {
        let config = AdcConfig::new()
            .calibration(true)
            .resolution(esp_idf_hal::adc::config::Resolution::Resolution12Bit);
        let mut channel_config = AdcChannelConfig::default();
        channel_config.resolution = Resolution::Resolution12Bit;
        channel_config.attenuation = esp_idf_hal::adc::attenuation::DB_11;

        let adc_driver = Rc::new(AdcDriver::new(adc).unwrap());

        let adc_channel_driver =
            AdcChannelDriver::new(adc_driver.clone(), adc_pin, &channel_config).unwrap();

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
