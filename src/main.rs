use sds011;
use sds011::SDS011;

use std::thread::sleep;
use std::time::Duration;
use std::error::Error;

mod dht11;
mod gpio;

use rppal::i2c::I2c;
use rppal::gpio::Gpio;
use ccs811;

use rppal::uart::{Parity, Uart};
use std::str;
use std::collections::HashMap;

use chrono;
use chrono::offset::Local;
use chrono::DateTime;
use std::time::SystemTime;

use std::{
    env,
    process
};
use  paho_mqtt as mqtt;
const DFLT_BROKER:&str = "tcp://192.168.168.167:1883";
const DFLT_CLIENT:&str = "rust_publish";
const DFLT_TOPIC:&str = "rust/mqtt";
const QOS:i32 = 1;
 
fn main(){
    let mut data_dict: HashMap<String, f32>  = HashMap::new();
    get_sds(&mut data_dict);
    get_coords(&mut data_dict).map_err(|err| println!("{:?}", err)).ok();
    get_dht(&mut data_dict);
    get_ccs811(&mut data_dict);
    mqttfn(&mut data_dict);
}

fn get_sds(data_dict: &mut HashMap<String, f32>) {
    let port = "/dev/ttyUSB0";
    let work_period_str = "1";
    let work_period = work_period_str.parse::<u8>().unwrap();

    match SDS011::new(port) {
        Ok(mut sensor) => {
            sensor.set_work_period(work_period).unwrap();

            while  !data_dict.contains_key("PM10") && !data_dict.contains_key("PM25") {
                if let Ok(m) = sensor.query() {
                    data_dict.insert(
                        "PM10".to_string(),
                        m.pm10,
                    );
                    data_dict.insert(
                        "PM25".to_string(),
                        m.pm25,
                    );
                }

                // sleep(Duration::from_secs(work_period as u64 * 5));
            }
        }
        //Err(e) => println!("{:?}", e.description),
        Err(e) => println!("{:?}", e),
    };
}

fn get_dht(data_dict: &mut HashMap<String, f32>) {
  let pin = 12;

  let mut sensor = dht11::create(pin).unwrap();
	
  while  !data_dict.contains_key("Temperature") && !data_dict.contains_key("Humidity") {
    match sensor.read_sensor() {
      Ok(reading) => {
        let (temp, humid) = reading;	  
                data_dict.insert(
                    "Temperature".to_string(),
                    temp.into(),
                );
                data_dict.insert(
                    "Humidity".to_string(),
                    humid.into(),
                );
            }
      Err(_e) => println!("Failed DHT read, waiting for succesfull read...")
    };
  }
} 
//=======================================================================================================


fn get_coords(data_dict: &mut HashMap<String, f32>) -> Result<(), Box<dyn Error>> {
    // Connect to the primary UART and configure it for 9600 baut, no
    // parity bit, 8 data bits and 1 stop bit.
    let mut uart = Uart::new(9600, Parity::None, 8, 1)?;

    // Configure read() to block until at least 1 byte is received.
    uart.set_read_mode(1, Duration::default())?;

    let mut buffer = [0u8; 1];
    let mut message = String::new();
    Ok(while  !data_dict.contains_key("Altitude") && !data_dict.contains_key("Latitude") && !data_dict.contains_key("Longtitude") {
        // Fill the buffer variable with any incoming data.
        if uart.read(&mut buffer)? > 0 {
                let s = match str::from_utf8(&mut buffer) {
                    Ok(v) => v,
                    Err(e) => "Empty Buffer",
                };
            
                if !s.trim().is_empty(){
                message += s;
                }else{
                    let split = message.split(",");
                    let vec: Vec<&str> =split.collect();
                     if vec[0] == "$GPRMC"{
                    let altitude_float: f32 =  vec[8].trim().parse().unwrap();
                    let latitude_float: f32 =  vec[3].trim().parse().unwrap();
                    let longtitude_float: f32 =  vec[5].trim().parse().unwrap();
                     data_dict.insert(
                        "Altitude".to_string(),
                        altitude_float,
                    );
                    data_dict.insert(
                        "Latitude".to_string(),
                        latitude_float,
                    );
                    data_dict.insert(
                        "Longtitude".to_string(),
                        longtitude_float,
                    );
                 }
                    message = String::new();
                }
            }
    })
}
//====================================================================================================

fn get_ccs811(data_dict: &mut HashMap<String, f32>) {
    let i2c = I2c::with_bus(1).expect("Couldn't start i2c. Is the interface enabled?");
    let wake_pin = Gpio::new().expect("Can not init gpio")
                            .get(17).expect("Could not attach to wake pin");
    wake_pin.into_output().set_low();

    let mut ccs811 = ccs811::new(i2c, None);

    match ccs811.begin() {
        Ok(()) => match ccs811.start(ccs811::MODE::Sec1) {
            Ok(()) => (),
            Err(error) => panic!("Could not start: {}", error)
        },
        Err(error) => panic!("Could not init the chip: {}", error)
    }
    sleep(Duration::from_secs(8));


    while  !data_dict.contains_key("CO2") && !data_dict.contains_key("VOC"){
        match ccs811.read() {
            Ok(data) => {
                data_dict.insert(
                    "CO2".to_string(),
                    data.e_co2.into(),
                );
                data_dict.insert(
                    "VOC".to_string(),
                    data.t_voc.into(),
                );
            },
            Err(error) => println!("Could not read data: {}", error)
        };
    }
}

fn mqttfn(data_dict: &mut HashMap<String, f32>){
    let host = env::args().nth(1).unwrap_or_else(||
        DFLT_BROKER.to_string()
    );

    // Define the set of options for the create.
    // Use an ID for a persistent session.
    let create_opts = mqtt::CreateOptionsBuilder::new()
        .server_uri(host)
        .client_id(DFLT_CLIENT.to_string())
        .finalize();

    // Create a client.
    let cli = mqtt::Client::new(create_opts).unwrap_or_else(|err| {
        println!("Error creating the client: {:?}", err);
        process::exit(1);
    });

    // Define the set of options for the connection.
    let conn_opts = mqtt::ConnectOptionsBuilder::new()
        .keep_alive_interval(Duration::from_secs(20))
        .clean_session(true)
        .finalize();

    // Connect and wait for it to complete or fail.
    if let Err(e) = cli.connect(conn_opts) {
        println!("Unable to connect:\n\t{:?}", e);
        process::exit(1);
    }
    // Create a message and publish it.
    // Publish messages to 'test' topics with sensordata.
    for (key_variable, value_variable) in data_dict.iter() {
    let system_time = SystemTime::now();
    let datetime: DateTime<Local> = system_time.into();
    println!("{}", datetime);
    let message: String = format!(" {:?}: {:?}, Time: {:?}",key_variable, value_variable, datetime);
    let msg = mqtt::Message::new(DFLT_TOPIC, message.clone(), QOS);
    println!("Publishing messages on the {:?} topic", DFLT_TOPIC);
    let _tok = cli.publish(msg);
    }

    // Disconnect from the broker.
    let _tok = cli.disconnect(None);
    println!("Disconnect from the broker");
    _tok.unwrap();
}
