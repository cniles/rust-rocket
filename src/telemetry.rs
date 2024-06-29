use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::{altimeter::AltimeterStats, battery::BatteryStats, datalink::ByteSerialize};

#[derive(Debug, Clone, Copy)]
pub struct Telemetry {
    pub time: u32,
    pub altitude: f32,
    pub temperature: f32,
    pub battery_voltage: f32,
}

impl Default for Telemetry {
    fn default() -> Self {
        Telemetry {
            time: 0,
            altitude: 0f32,
            temperature: 0f32,
            battery_voltage: 0f32,
        }
    }
}

impl From<(AltimeterStats, BatteryStats)> for Telemetry {
    fn from(value: (AltimeterStats, BatteryStats)) -> Self {
        Self {
            time: 0,
            altitude: value.0.altitude as f32,
            temperature: value.0.temperature as f32,
            battery_voltage: value.1.voltage,
        }
    }
}

impl ByteSerialize<Telemetry> for Telemetry {
    fn as_bytes(&self, buffer: &mut [u8]) -> Result<(), ()> {
        let mut buf = BytesMut::with_capacity(std::mem::size_of::<Telemetry>());

        buf.put_u32_le(self.time);
        buf.put_f32_le(self.altitude);
        buf.put_f32_le(self.temperature);
        buf.put_f32_le(self.battery_voltage);

        buffer[..buf.len()].copy_from_slice(&buf);

        Ok(())
    }

    fn from_bytes(buffer: &[u8]) -> Result<Telemetry, ()> {
        let mut buf = Bytes::copy_from_slice(buffer);

        Ok::<Telemetry, ()>(Telemetry {
            time: buf.get_u32_le(),
            altitude: buf.get_f32_le(),
            temperature: buf.get_f32_le(),
            battery_voltage: buf.get_f32_le(),
        })
    }
}
