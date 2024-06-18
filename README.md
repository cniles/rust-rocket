# Issues

- Need to reproduce: Telemetry stopped sending on flight with drone. Base station was still receiving updates.
The updates all held the same data.  The implication is that the rocket computer was querying the sensor but it was not
able to successfully furnish new readings to be used hence it kept sending old data.

- Wifi interface to base station.

# Todo
Need a black box.  What is happening each tick?  Records messages received, sent, and errors. 
