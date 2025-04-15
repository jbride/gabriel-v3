use std::{env, thread, time::Duration};
use log::{info, error, warn};
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
    let mut cfg = Config::new(Network::Mainnet);
    //cfg = cfg.with_fee_estimation(false);
    
    // Add specific nodes to connect to
    // Format: "ip:port" or "hostname:port"
    let connect_nodes = env::var("NAKAMOTO_CONNECT_NODES")
        .unwrap_or_else(|_| String::new());
    
    // Parse comma-separated list of nodes
    for node in connect_nodes.split(',') {
        let node = node.trim();
        if !node.is_empty() {
            match node.parse() {
                Ok(addr) => {
                    info!("Adding connection to node: {}", node);
                    cfg.connect.push(addr);
                },
                Err(e) => {
                    warn!("Failed to parse node address '{}': {}", node, e);
                }
            }
        }
    }
    
    // Log connection strategy
    if cfg.connect.is_empty() {
        info!("No specific nodes configured. Will discover peers from the network.");
    } else {
        info!("Will connect to {} specified node(s)", cfg.connect.len());
    }
    
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
    
    // Wait for peers to connect with timeout handling
    info!("Waiting for peers to connect...");
    
    // Read the Nakamoto client peer count from the environment variable, defaulting to 4 if not set
    let peer_count: usize = env::var("NAKAMOTO_PEER_COUNT")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(4);
    info!("Waiting for {} peer(s) to connect...", peer_count);
    
    // Add timeout for peer connection
    let timeout_duration = Duration::from_secs(60); // 60 second timeout
    let start_time = std::time::Instant::now();
    
    loop {
        match header_handle.wait_for_peers(peer_count, Services::Chain) {
            Ok(_) => {
                info!("Connected to {} peers", peer_count);
                break;
            },
            Err(e) => {
                if start_time.elapsed() > timeout_duration {
                    error!("Timeout waiting for peers: {}", e);
                    return Err(format!("Timeout waiting for peers: {}", e).into());
                }
                info!("Waiting for peers... ({:?} elapsed)", start_time.elapsed());
                thread::sleep(Duration::from_secs(5));
            }
        }
    }
    
    // Get the current tip height
    let (mut tip_height, _) = match header_handle.get_tip() {
        Ok(tip) => tip,
        Err(e) => {
            error!("Failed to get tip: {}", e);
            return Err(format!("Failed to get tip: {}", e).into());
        }
    };
    info!("Current blockchain tip height: {}", tip_height);
    
    // Process blocks from specified start height to tip
    let start_height = env::var("NAKAMOTO_BLOCK_HEIGHT_START")
        .ok()
        .and_then(|val| val.parse::<u64>().ok())
        .unwrap_or(0);
    
    info!("Starting to fetch blocks from height {}...", start_height);
    let mut current_height = start_height;
    
    while current_height <= tip_height {
        info!("Fetching block at height {}...", current_height);
        
        // Get the block header at the current height with error handling
        let block_header = match header_handle.get_block_by_height(current_height) {
            Ok(Some(header)) => header,
            Ok(None) => {
                error!("No block found at height {}", current_height);
                current_height += 1;
                continue;
            },
            Err(e) => {
                error!("Error fetching block at height {}: {}", current_height, e);
                return Err(format!("Error fetching block at height {}: {}", current_height, e).into());
            }
        };
        
        let block_hash = block_header.block_hash();
        info!("Block {} hash: {}", current_height, block_hash);
        
        // Request the full block with error handling
        if let Err(e) = header_handle.get_block(&block_hash) {
            error!("Error requesting block {}: {}", block_hash, e);
            return Err(format!("Error requesting block {}: {}", block_hash, e).into());
        }
        
        // Wait a moment to avoid overwhelming the network
        thread::sleep(Duration::from_millis(100));
        
        // Move to the next block
        current_height += 1;
        
        // Periodically check if the tip has advanced
        if current_height % 100 == 0 {
            match header_handle.get_tip() {
                Ok((new_tip, _)) => {
                    if new_tip > tip_height {
                        info!("New tip detected: {} (was {})", new_tip, tip_height);
                        tip_height = new_tip;
                    }
                },
                Err(e) => {
                    error!("Error checking tip: {}", e);
                    // Continue anyway, not fatal
                }
            }
        }
    }
    
    info!("Finished processing all blocks up to height {}", tip_height);
    
    // Shutdown the client
    info!("Shutting down client...");
    if let Err(e) = header_handle.shutdown() {
        error!("Error shutting down client: {}", e);
        return Err(format!("Error shutting down client: {}", e).into());
    }
    
    // Wait for the client thread to finish
    match client_thread.join() {
        Ok(_) => info!("Client thread joined successfully"),
        Err(e) => error!("Client thread panicked: {:?}", e),
    }
    
    info!("Block fetcher completed successfully");
    Ok(())
}

