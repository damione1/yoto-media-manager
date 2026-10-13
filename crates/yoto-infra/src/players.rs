use rumqttc::{AsyncClient, Event, MqttOptions, Outgoing, Packet, QoS, Transport};
use serde_json::{Value, json};
use std::time::Duration;
use yoto_domain::*;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CommandResult {
    Sent,
    Accepted,
    Observed { status: Value },
}
pub fn encode(command: &PlayerCommand) -> Result<(&'static str, Value)> {
    Ok(match command{
 PlayerCommand::Volume{value}=>("volume/set",json!({"volume":value.value()})),
 PlayerCommand::Ambient{r,g,b}=>("ambients/set",json!({"r":r,"g":g,"b":b})),
 PlayerCommand::SleepTimer{seconds}=>("sleep-timer/set",json!({"seconds":seconds})),
 PlayerCommand::Reboot=>("reboot",json!({})),
 PlayerCommand::Start{card,chapter,track,seconds}=>{let mut v=json!({"uri":format!("https://yoto.io/{card}"),"secondsIn":seconds});if let Some(c)=chapter{v["chapterKey"]=json!(c.as_str());}
 if let Some(t)=track{v["trackKey"]=json!(t.as_str());}("card/start",v)},
 PlayerCommand::Stop=>("card/stop",json!({})),PlayerCommand::Pause=>("card/pause",json!({})),PlayerCommand::Resume=>("card/resume",json!({})),
 PlayerCommand::BluetoothOn=>return Err(Failure::Compatibility("The published Bluetooth action/mode values are incomplete. Activation needs a confirmed model-specific contract.".into())),
 PlayerCommand::BluetoothOff=>("bluetooth/off",json!({})),
 PlayerCommand::BluetoothDeleteBond{..}|PlayerCommand::BluetoothConnect{..}=>return Err(Failure::Compatibility("The Bluetooth address payload is not specified by the public contract.".into())),
 PlayerCommand::BluetoothDisconnect=>("bluetooth/disconnect",json!({})),PlayerCommand::BluetoothState=>("bluetooth/state",json!({})),
 PlayerCommand::DisplayPreview{icon}=>("display/preview",json!({"uri":format!("yoto:#{icon}"),"timeout":5,"animated":0})),
 PlayerCommand::Events=>("events/request",json!({})),PlayerCommand::Status=>("status/request",json!({}))})
}
/// Ephemeral command session. A disconnect never queues physical commands for replay.
pub async fn send(device: &Device, token: &str, command: &PlayerCommand) -> Result<CommandResult> {
    if !device.online {
        return Err(Failure::Validation(
            "The selected player is offline.".into(),
        ));
    }
    availability(device, command)?;
    let mut options = MqttOptions::new(
        format!("DASH{}", device.id),
        "wss://aqrphjqbp3u2z-ats.iot.eu-west-2.amazonaws.com/mqtt",
        443,
    );
    options.set_transport(Transport::wss_with_default_config());
    options.set_keep_alive(Duration::from_secs(30));
    options.set_credentials(
        format!(
            "{}?x-amz-customauthorizer-name=PublicJWTAuthorizer",
            device.id
        ),
        token,
    );
    send_transport(device, command, options).await
}
async fn send_transport(
    device: &Device,
    command: &PlayerCommand,
    options: MqttOptions,
) -> Result<CommandResult> {
    let (suffix, payload) = encode(command)?;
    let (client, mut events) = AsyncClient::new(options, 8);
    let prefix = format!("device/{}", device.id);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut sent = false;
    let mut queued = false;

    loop {
        let event = match tokio::time::timeout_at(deadline, events.poll()).await {
            Err(_) => {
                return if sent {
                    Ok(CommandResult::Sent)
                } else {
                    Err(Failure::Connection)
                };
            }
            Ok(Err(_)) => {
                return Err(if sent {
                    Failure::Ambiguous
                } else {
                    Failure::Connection
                });
            }
            Ok(Ok(event)) => event,
        };
        match event {
            Event::Incoming(Packet::ConnAck(_)) => {
                client
                    .subscribe(format!("{prefix}/response"), QoS::AtMostOnce)
                    .await
                    .map_err(|_| Failure::Connection)?;
                client
                    .subscribe(format!("{prefix}/data/status"), QoS::AtMostOnce)
                    .await
                    .map_err(|_| Failure::Connection)?;
                client
                    .subscribe(format!("{prefix}/data/events"), QoS::AtMostOnce)
                    .await
                    .map_err(|_| Failure::Connection)?;
            }
            Event::Incoming(Packet::SubAck(_)) if !queued => {
                client
                    .publish(
                        format!("{prefix}/command/{suffix}"),
                        QoS::AtMostOnce,
                        false,
                        serde_json::to_vec(&payload).map_err(crate::storage)?,
                    )
                    .await
                    .map_err(|_| Failure::Connection)?;
                queued = true;
            }
            Event::Outgoing(Outgoing::Publish(_)) if queued => {
                sent = true;
            }
            Event::Incoming(Packet::Publish(p)) => {
                if p.retain || !sent {
                    continue;
                }
                if p.payload.len() > 64_000 {
                    return Err(Failure::Compatibility("Player response too large".into()));
                }
                let status: Value = serde_json::from_slice(&p.payload)
                    .map_err(|_| Failure::Compatibility("Invalid player response".into()))?;
                if p.topic == format!("{prefix}/response") {
                    let body = status
                        .get("status")
                        .and_then(|s| s.get("req_body"))
                        .and_then(Value::as_str)
                        .and_then(|b| serde_json::from_str::<Value>(b).ok());
                    if response_matches(&status, command, &payload)
                        && body.as_ref() == Some(&payload)
                    {
                        if status
                            .get("status")
                            .and_then(Value::as_object)
                            .is_some_and(|s| s.values().any(|v| v.as_str() == Some("FAIL")))
                        {
                            return Err(Failure::Validation(
                                "The player rejected the command".into(),
                            ));
                        }
                        client.disconnect().await.map_err(|_| Failure::Connection)?;
                        return Ok(CommandResult::Accepted);
                    }
                } else if sent
                    && matches!(
                        command,
                        PlayerCommand::Events
                            | PlayerCommand::Status
                            | PlayerCommand::BluetoothState
                    )
                    && observation_matches(command, &p.topic, &prefix, p.retain)
                {
                    client.disconnect().await.map_err(|_| Failure::Connection)?;
                    return Ok(CommandResult::Observed { status });
                }
            }
            Event::Incoming(_) | Event::Outgoing(_) => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return if sent {
                Ok(CommandResult::Sent)
            } else {
                Err(Failure::Connection)
            };
        }
    }
}

fn response_matches(status: &Value, command: &PlayerCommand, payload: &Value) -> bool {
    let resource = match command {
        PlayerCommand::Volume { .. } => "volume",
        PlayerCommand::Ambient { .. } => "ambients",
        PlayerCommand::SleepTimer { .. } => "sleep-timer",
        PlayerCommand::Reboot => "reboot",
        PlayerCommand::DisplayPreview { .. } => "display",
        PlayerCommand::Events => "events",
        PlayerCommand::Status => "status",
        // The public card/Bluetooth response uses a shared resource and no command discriminator.
        // Even identical {} bodies cannot prove which action was accepted; report actual Sent only.
        PlayerCommand::Start { .. }
        | PlayerCommand::Stop
        | PlayerCommand::Pause
        | PlayerCommand::Resume
        | PlayerCommand::BluetoothOn
        | PlayerCommand::BluetoothOff
        | PlayerCommand::BluetoothDeleteBond { .. }
        | PlayerCommand::BluetoothConnect { .. }
        | PlayerCommand::BluetoothDisconnect
        | PlayerCommand::BluetoothState => return false,
    };
    let Some(status) = status.get("status") else {
        return false;
    };
    matches!(
        status.get(resource).and_then(Value::as_str),
        Some("OK" | "FAIL")
    ) && status
        .get("req_body")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .as_ref()
        == Some(payload)
}
fn observation_matches(command: &PlayerCommand, topic: &str, prefix: &str, retained: bool) -> bool {
    if retained {
        return false;
    }
    match command {
        PlayerCommand::Status => topic == format!("{prefix}/data/status"),
        PlayerCommand::Events => topic == format!("{prefix}/data/events"),
        PlayerCommand::Volume { .. }
        | PlayerCommand::Ambient { .. }
        | PlayerCommand::SleepTimer { .. }
        | PlayerCommand::Reboot
        | PlayerCommand::Start { .. }
        | PlayerCommand::Stop
        | PlayerCommand::Pause
        | PlayerCommand::Resume
        | PlayerCommand::BluetoothOn
        | PlayerCommand::BluetoothOff
        | PlayerCommand::BluetoothDeleteBond { .. }
        | PlayerCommand::BluetoothConnect { .. }
        | PlayerCommand::BluetoothDisconnect
        | PlayerCommand::BluetoothState
        | PlayerCommand::DisplayPreview { .. } => false,
    }
}
pub fn availability(device: &Device, command: &PlayerCommand) -> Result<()> {
    match command {
        PlayerCommand::Ambient{..}|PlayerCommand::DisplayPreview{..}|PlayerCommand::BluetoothOff|PlayerCommand::BluetoothDisconnect => {
            if device.product.as_deref()!=Some("v2") || matches!(command,PlayerCommand::BluetoothOff|PlayerCommand::BluetoothDisconnect) { return Err(Failure::Compatibility(format!("No verified public capability mapping for {:?} on model {:?}. Model-specific commands are unavailable until that mapping is established.",command,device.product))); }
        }
        PlayerCommand::BluetoothState=>return Err(Failure::Compatibility("The Bluetooth-state response topic/correlation is not documented; a current observation cannot be confirmed.".into())),
        PlayerCommand::BluetoothOn|PlayerCommand::BluetoothDeleteBond{..}|PlayerCommand::BluetoothConnect{..}=>{ encode(command)?; }
        PlayerCommand::Volume{..}|PlayerCommand::SleepTimer{..}|PlayerCommand::Reboot|PlayerCommand::Start{..}|PlayerCommand::Stop|PlayerCommand::Pause|PlayerCommand::Resume|PlayerCommand::Events|PlayerCommand::Status=>{},
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acknowledgements_match_identity_body_and_observation_kind() {
        let pause = json!({"status":{"card":"OK","req_body":"{}"}});
        assert!(!response_matches(&pause, &PlayerCommand::Pause, &json!({})));
        assert!(!response_matches(
            &pause,
            &PlayerCommand::Reboot,
            &json!({})
        ));
        assert!(response_matches(
            &json!({"status":{"reboot":"OK","req_body":"{}"}}),
            &PlayerCommand::Reboot,
            &json!({})
        ));
        assert!(!observation_matches(
            &PlayerCommand::Status,
            "device/x/data/events",
            "device/x",
            false
        ));
        assert!(!observation_matches(
            &PlayerCommand::Status,
            "device/x/data/status",
            "device/x",
            true
        ));
        assert!(observation_matches(
            &PlayerCommand::Status,
            "device/x/data/status",
            "device/x",
            false
        ));
    }
}
#[cfg(test)]
mod transport_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn packet(stream: &mut tokio::net::TcpStream) -> (u8, Vec<u8>) {
        let header = stream.read_u8().await.unwrap();
        let mut size = 0;
        let mut mult = 1;
        loop {
            let b = stream.read_u8().await.unwrap();
            size += (b & 127) as usize * mult;
            if b & 128 == 0 {
                break;
            }
            mult *= 128;
        }
        let mut body = vec![0; size];
        stream.read_exact(&mut body).await.unwrap();
        (header, body)
    }
    async fn publish(stream: &mut tokio::net::TcpStream, topic: &str, value: Value, retain: bool) {
        let mut body = vec![];
        body.extend((topic.len() as u16).to_be_bytes());
        body.extend(topic.as_bytes());
        body.extend(serde_json::to_vec(&value).unwrap());
        let mut wire = vec![0x30 | u8::from(retain)];
        let mut len = body.len();
        loop {
            let b = (len % 128) as u8;
            len /= 128;
            wire.push(b | if len > 0 { 128 } else { 0 });
            if len == 0 {
                break;
            }
        }
        wire.extend(body);
        stream.write_all(&wire).await.unwrap();
    }
    #[tokio::test]
    async fn real_transport_ignores_other_command_and_retained_or_unrelated_observations() {
        for command in [PlayerCommand::Reboot, PlayerCommand::Status] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let broker = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                assert_eq!(packet(&mut stream).await.0 >> 4, 1);
                stream.write_all(&[0x20, 2, 0, 0]).await.unwrap();
                loop {
                    let (header, body) = packet(&mut stream).await;
                    match header >> 4 {
                        8 => stream
                            .write_all(&[0x90, 3, body[0], body[1], 0])
                            .await
                            .unwrap(),
                        3 => break,
                        other => panic!("Unexpected MQTT packet {other}"),
                    }
                }
                publish(
                    &mut stream,
                    "device/x/response",
                    json!({"status":{"card":"OK","req_body":"{}"}}),
                    false,
                )
                .await;
                publish(
                    &mut stream,
                    "device/x/data/events",
                    json!({"stale":1}),
                    false,
                )
                .await;
                publish(
                    &mut stream,
                    "device/x/data/status",
                    json!({"stale":2}),
                    true,
                )
                .await;
                tokio::time::sleep(Duration::from_millis(30)).await;
                publish(
                    &mut stream,
                    "device/x/data/status",
                    json!({"current":3}),
                    false,
                )
                .await;
                publish(
                    &mut stream,
                    "device/x/response",
                    json!({"status":{"reboot":"OK","req_body":"{}"}}),
                    false,
                )
                .await;
                let mut end = [0; 2];
                let _ = stream.read(&mut end).await;
            });
            let device = Device {
                id: DeviceId::new("x").unwrap(),
                name: "Test".into(),
                online: true,
                firmware: None,
                product: Some("future-unknown".into()),
                extra: Default::default(),
            };
            assert!(availability(&device, &PlayerCommand::Ambient { r: 1, g: 2, b: 3 }).is_err());
            let options = MqttOptions::new("DASHx", "127.0.0.1", port);
            let result = send_transport(&device, &command, options).await.unwrap();
            match command {
                PlayerCommand::Reboot => assert!(matches!(result, CommandResult::Accepted)),
                PlayerCommand::Status => assert!(
                    matches!(result,CommandResult::Observed{status} if status==json!({"current":3}))
                ),
                PlayerCommand::Volume { .. }
                | PlayerCommand::Ambient { .. }
                | PlayerCommand::SleepTimer { .. }
                | PlayerCommand::Start { .. }
                | PlayerCommand::Stop
                | PlayerCommand::Pause
                | PlayerCommand::Resume
                | PlayerCommand::BluetoothOn
                | PlayerCommand::BluetoothOff
                | PlayerCommand::BluetoothDeleteBond { .. }
                | PlayerCommand::BluetoothConnect { .. }
                | PlayerCommand::BluetoothDisconnect
                | PlayerCommand::BluetoothState
                | PlayerCommand::DisplayPreview { .. }
                | PlayerCommand::Events => unreachable!(),
            }
            tokio::time::timeout(Duration::from_secs(2), broker)
                .await
                .unwrap()
                .unwrap();
        }
    }
}
