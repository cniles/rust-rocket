# Rust Rocket

Send commands and receive telemetry from an ESP32, typically in the nosecone of a model rocket.

Commands are sent from a basestation made with a Cheap Yellow Display (CYD) using ESP-NOW.  The flight computer is based around a TinyPICO device.

The flight computer reads from a BMP-390 altimeter and cleaned up with a simple Kalman filter before transmitting telemetry
to the basestation.
