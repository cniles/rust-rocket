use esp_idf_hal::{
    adc::{
        oneshot::{config::AdcChannelConfig, AdcChannelDriver, AdcDriver},
        Resolution,
    },
    gpio::{ADCPin, Input, InputPin, Pin, PinDriver},
    peripheral::Peripheral,
    sys::EspError,
};

pub struct Battery<C, T>
where
    C: Pin + InputPin,
    T: ADCPin,
{
    charge_pin: PinDriver<'static, C, Input>,
    adc_channel_driver: AdcChannelDriver<'static, T, AdcDriver<'static, T::Adc>>,
}

pub struct BatteryStats {
    pub charging: bool,
    pub voltage: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum BatteryError<V> {
    ChargeError(V),
    VoltageError(V),
}

impl<C, T> Battery<C, T>
where
    C: Pin + InputPin,
    T: ADCPin,
{
    pub fn new(
        charge_pin: impl Peripheral<P = C> + 'static,
        adc: impl Peripheral<P = T::Adc> + 'static,
        adc_pin: T,
    ) -> Result<Self, BatteryError<EspError>> {
        let charge_pin = PinDriver::input(charge_pin).map_err(BatteryError::ChargeError)?;

        let channel_config = AdcChannelConfig {
            resolution: Resolution::Resolution12Bit,
            attenuation: esp_idf_hal::adc::attenuation::DB_11,
            calibration: true,
            ..AdcChannelConfig::default()
        };

        let adc_driver = AdcDriver::new(adc).map_err(BatteryError::VoltageError)?;

        let adc_channel_driver = AdcChannelDriver::new(adc_driver, adc_pin, &channel_config)
            .map_err(BatteryError::VoltageError)?;

        Ok(Self {
            charge_pin,
            adc_channel_driver,
        })
    }

    pub fn charging(&mut self) -> bool {
        let mut charging = 0;

        for _i in 0..10 {
            if self.charge_pin.is_high() {
                charging = charging + 1;
            }
        }

        charging == 0
    }

    pub fn voltage(&mut self) -> Result<f64, BatteryError<EspError>> {
        let v = self
            .adc_channel_driver
            .read()
            .map_err(BatteryError::VoltageError)?;

        //
        // the voltage adc is divided by a 442k and 160k resitor network.
        // We should be receiving a reading that is 160k/602k (~0.27) of Vbat.
        let scale = 160f64 / 602f64;

        Ok((v as f64) / 4095f64 / scale * 3.7f64)
    }

    pub fn stats(&mut self) -> Result<BatteryStats, BatteryError<EspError>> {
        Ok(BatteryStats {
            charging: self.charging(),
            voltage: self.voltage()? as f32,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_foo() {
        assert!(false);
    }
}
