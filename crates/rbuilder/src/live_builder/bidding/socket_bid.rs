use alloy_primitives::U256;
use alloy_rpc_types_beacon::BlsPublicKey;
use futures::{SinkExt, StreamExt};
use revm_primitives::{Address, B256};
use ssz::Decode;
use ssz_derive::{Encode, Decode};
use tokio::sync::watch;
use std::time::Duration;
use tracing::warn;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, WebSocketStream, MaybeTlsStream};
use tokio::net::TcpStream;

#[derive(Debug, Clone, Serialize, Deserialize, Decode, Encode)]
pub struct TopBidUpdate {
    pub timestamp: u64,
    pub slot: u64,
    pub block_number: u64,
    pub block_hash: B256,
    pub parent_hash: B256,
    pub builder_pubkey: BlsPublicKey,
    pub fee_recipient: Address,
    pub value: U256,
}
impl TopBidUpdate {
    fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if data.len() != 188 {
            return Err("Invalid data length".into());
        }
        Ok(Self::from_ssz_bytes(data).unwrap())
    }
}

impl std::fmt::Display for TopBidUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TopBidUpdate {{ timestamp: {}, slot: {}, block_number: {}, block_hash: 0x{:x}, parent_hash: 0x{:x}, builder_pubkey: 0x{:x}, fee_recipient: 0x{:x}, value: {} }}",
               self.timestamp, self.slot, self.block_number, self.block_hash, self.parent_hash, self.builder_pubkey, self.fee_recipient, self.value)
    }
}


#[derive(Debug, Clone)]
pub struct TopBidWatcher {
    best_bid_sender: watch::Sender<U256>,
}

impl TopBidWatcher {
    pub fn new() -> (Self, watch::Receiver<U256>) {
        let (sender, receiver) = watch::channel(U256::ZERO);
        (Self { best_bid_sender: sender }, receiver)
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let url = "ws://relay-builders-eu.ultrasound.money/ws/v1/top_bid";

        loop {
            match connect_async(url).await {
                Ok((ws_stream, _)) => {
                    if let Err(e) = self.handle_connection(ws_stream).await {
                        warn!("WebSocket error: {:?}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to connect: {:?}", e);
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn handle_connection(&self, mut ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Result<(), Box<dyn std::error::Error>> {
        let mut ping_interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                Some(msg) = ws_stream.next() => {
                    match msg? {
                        Message::Binary(data) => {
                            match TopBidUpdate::from_bytes(&data) {
                                Ok(update) => {

                                    self.best_bid_sender.send(update.value)?;
                                },
                                Err(e) => warn!("Failed to decode TopBidUpdate: {:?}", e),
                            }
                        },

                        Message::Ping(_) => {
                            ws_stream.send(Message::Pong(vec![])).await?;
                        },
                        Message::Close(_) => {
                            return Ok(());
                        },
                        _ => {},
                    }
                }
                _ = ping_interval.tick() => {
                    ws_stream.send(Message::Ping(vec![])).await?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;
    use tracing::{debug, info, Level};
    use tracing_subscriber::FmtSubscriber;

    #[tokio::test]
    async fn test_top_bid_watcher() {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::DEBUG)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set tracing subscriber");

        let (top_bid_watcher, mut best_bid_receiver) = TopBidWatcher::new();

        let handle = tokio::spawn(async move {
            if let Err(e) = top_bid_watcher.run().await {
                panic!("TopBidWatcher run failed: {:?}", e);
            }
        });

        let mut updates_count = 0;
        let max_updates = 1000u32;
        let test_duration = Duration::from_secs(300);

        let result = timeout(test_duration, async {
            loop {
                tokio::select! {
                    _ = best_bid_receiver.changed() => {
                        let new_value = *best_bid_receiver.borrow();
                        debug!("Received new bid value: {:?}", new_value);
                        assert_ne!(new_value, U256::ZERO);
                        updates_count += 1;
                        if updates_count >= max_updates {
                            break;
                        }
                    }
                    else => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }).await;

        match result {
            Ok(_) => {
                info!("Test completed successfully. Received {} updates.", updates_count);
                assert_eq!(updates_count, max_updates, "Did not receive expected number of updates");
            },
            Err(_) => panic!("Test timed out after {:?}", test_duration),
        }

        // Ensure the TopBidWatcher is still running
        assert!(!handle.is_finished());

        // Stop the TopBidWatcher
        handle.abort();
    }


    // #[test]
    // fn test_top_bid_update_from_bytes() {
    //     let data = [89, 137, 80, 112, 145, 1, 0, 0, 84, 53, 149, 0, 0, 0, 0, 0, 237, 225, 57, 1, 0, 0, 0, 0, 158, 153, 53, 39, 28, 85, 57, 172, 184, 26, 174, 136, 54, 169, 147, 37, 210, 42, 212, 149, 126, 28, 17, 152, 48, 13, 88, 128, 106, 176, 251, 38, 185, 77, 173, 202, 235, 188, 88, 202, 253, 90, 25, 199, 224, 54, 166, 10, 77, 209, 142, 59, 54, 134, 207, 235, 138, 83, 120, 56, 96, 168, 134, 71, 171, 132, 123, 239, 229, 155, 94, 255, 255, 161, 47, 71, 172, 244, 76, 191, 142, 248, 117, 231, 200, 145, 164, 238, 158, 156, 72, 50, 84, 207, 154, 85, 245, 237, 104, 142, 67, 255, 91, 198, 205, 146, 118, 233, 144, 145, 146, 27, 59, 100, 33, 106, 209, 165, 143, 97, 83, 139, 79, 161];

    //     let update = TopBidUpdate::from_bytes(&data).unwrap();
    //     println!("{}", update);

    //     assert_eq!(update.timestamp, 80001234567985);
    //     assert_eq!(update.slot, 9760628);
    //     assert_eq!(update.block_number, 16820725);
    //     assert_eq!(update.value, U256::from_str_radix("6153518710975100110", 10).unwrap());
    // }

}
