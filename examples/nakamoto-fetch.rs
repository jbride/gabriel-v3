use std::{env, thread, time::Duration};
use log::{info, error};
use env_logger;

use nakamoto::client::{
    network::{Network, Services},
    traits::Handle,
    Client, Config,
};

/// The network reactor we're going to use.
type Reactor = nakamoto::net::poll::Reactor<std::net::TcpStream>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("Starting Nakamoto block fetcher...");
    
    // Create a client configuration for mainnet
    let cfg = Config::new(Network::Mainnet);
    
    // Create a client using the network reactor
    let client = Client::<Reactor>::new()?;
    let header_handle = client.handle();
    let block_handle = client.handle();
    
    // Spawn the client thread
    let client_thread = thread::spawn(move || {
        match client.run(cfg) {
            Ok(_) => info!("Client terminated successfully"),
            Err(e) => error!("Client error: {}", e),
        }
    });
    
    // Wait for peers to connect
    info!("Waiting for peers to connect...");
    let peer_count = 4; // Number of peers to wait for
    header_handle.wait_for_peers(peer_count, Services::Chain)?;
    info!("Connected to {} peers", peer_count);
    
    // Get the current tip height
    let (mut tip_height, _) = header_handle.get_tip()?;
    info!("Current blockchain tip height: {}", tip_height);
    
    // Process blocks from genesis (height 0) to tip
    info!("Starting to fetch blocks from genesis...");
    let mut current_height = 0;
    
    while current_height <= tip_height {
        info!("Fetching block at height {}...", current_height);
        
        // Get the block header at the current height
        let block_header = match header_handle.get_block_by_height(current_height)? {
            Some(header) => header,
            None => {
                error!("No block found at height {}", current_height);
                current_height += 1;
                continue;
            }
        };
        
        let block_hash = block_header.block_hash();
        info!("Block {} hash: {}", current_height, block_hash);
        
        // Request the full block
        header_handle.get_block(&block_hash)?;
        
        // Wait a moment to avoid overwhelming the network
        thread::sleep(Duration::from_millis(100));
        
        // Move to the next block
        current_height += 1;
        
        // Periodically check if the tip has advanced
        if current_height % 100 == 0 {
            let (new_tip, _) = header_handle.get_tip()?;
            if new_tip > tip_height {
                info!("New tip detected: {} (was {})", new_tip, tip_height);
                tip_height = new_tip;
            }
        }
    }
    
    info!("Finished processing all blocks up to height {}", tip_height);
    
    // Shutdown the client
    info!("Shutting down client...");
    header_handle.shutdown()?;
    
    // Wait for the client thread to finish
    client_thread.join().expect("Client thread panicked");
    
    info!("Block fetcher completed successfully");
    Ok(())
}

