#include <Wire.h>
#include <Adafruit_Sensor.h>
#include <Adafruit_ADXL345_U.h>

const int BAUD_RATE = 9600;
const int DELAY_MS = 100;

Adafruit_ADXL345_Unified accel = Adafruit_ADXL345_Unified();

void setup(void)
{
	Serial.begin(BAUD_RATE);
	if(!accel.begin())
	{
		Serial.println("!! Kein Sensor Gefunden !!");
		while(1);
	}
}

void loop(void)
{
	sensors_event_t event;
	accel.getEvent(&event);

	// serialisiere die Beschleunigungsdaten als JSON-Array
	Serial.print("[");
	Serial.print(event.acceleration.x);
	Serial.print(",");
	Serial.print(event.acceleration.y);
	Serial.print(",");
	Serial.print(event.acceleration.z);
	Serial.println("]");

	delay(DELAY_MS);
}
