use clap::Parser;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Message};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use std::time::Duration;
use tokio::signal;
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio_stream::StreamExt;
use tracing::{error, info};

use mir_azure::azure::AzureDocumentClient;
use mir_azure::worker::{process_request_streaming, WorkerArgs, WorkerContext};
use nauron_contracts::MirEvent;

const KAFKA_MAX_POLL_INTERVAL_MS: &str = "86400000";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = WorkerArgs::parse();
    info!("Starting MIR Azure Worker");

    let azure_client = AzureDocumentClient::new(
        args.azure_di_endpoint.clone(),
        args.azure_di_key.clone(),
        args.azure_di_model_id.clone(),
        args.azure_di_api_version.clone(),
    )?;

    let context = WorkerContext::new(args.clone(), azure_client).await?;
    let config = context.config();

    let consumer = create_consumer(config)?;
    consumer.subscribe(&[&config.request_topic])?;

    let producer = create_producer(config)?;

    let mut stream = consumer.stream();
    info!("Subscribed to {}", config.request_topic);

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Shutdown signal received");
                break;
            }
            maybe_message = stream.next() => match maybe_message {
                Some(Ok(message)) => {
                    handle_message(&context, &producer, &consumer, message).await;
                }
                Some(Err(err)) => error!("Kafka consumer error: {}", err),
                None => break,
            }
        }
    }

    Ok(())
}

async fn handle_message(
    ctx: &WorkerContext,
    producer: &FutureProducer,
    consumer: &StreamConsumer,
    message: BorrowedMessage<'_>,
) {
    let payload = match message.payload_view::<str>() {
        Some(Ok(text)) => text,
        _ => {
            error!("Invalid payload received, skipping. Raw message may not be valid UTF-8.");
            let _ = consumer.commit_message(&message, CommitMode::Async);
            return;
        }
    };

    let request = match serde_json::from_str::<nauron_contracts::MirRequest>(payload) {
        Ok(req) => req,
        Err(err) => {
            error!(
                "Failed to deserialize request: {}. Raw payload: {}",
                err, payload
            );
            let _ = consumer.commit_message(&message, CommitMode::Async);
            return;
        }
    };

    info!(
        "Processing job {} for context {}",
        request.job_id, request.context_id
    );

    let job_key = request.job_id.to_string();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let sender = tokio::spawn(publish_events(
        producer.clone(),
        ctx.config().progress_topic.clone(),
        ctx.config().result_topic.clone(),
        job_key,
        event_rx,
    ));

    let output = process_request_streaming(&request, ctx, |event| {
        if event_tx.send(event).is_err() {
            error!("Failed to enqueue event for kafka publish");
        }
    })
    .await;
    drop(event_tx);

    if let Err(err) = sender.await {
        error!("Kafka event publisher task failed: {}", err);
    };

    info!(
        job_id = %request.job_id,
        events = output.events.len(),
        "MIR Azure job events published"
    );

    let _ = consumer.commit_message(&message, CommitMode::Async);
}

async fn publish_events(
    producer: FutureProducer,
    progress_topic: String,
    result_topic: String,
    key: String,
    mut events: UnboundedReceiver<MirEvent>,
) {
    while let Some(event) = events.recv().await {
        let topic = match &event {
            MirEvent::Progress(_) => &progress_topic,
            MirEvent::Result(_) => &result_topic,
        };

        if let Err(err) = send_event(&producer, topic, &event, &key).await {
            error!("Failed to send event to kafka: {}", err);
        }
    }
}

async fn send_event(
    producer: &FutureProducer,
    topic: &str,
    event: &MirEvent,
    key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let payload = serde_json::to_string(event)?;
    producer
        .send(
            FutureRecord::to(topic).payload(&payload).key(key),
            Duration::from_secs(0),
        )
        .await
        .map_err(|(err, _)| err)?;
    Ok(())
}

fn create_consumer(config: &WorkerArgs) -> Result<StreamConsumer, rdkafka::error::KafkaError> {
    let mut builder = base_client_config(config);
    builder
        .set("group.id", &config.group_id)
        .set("enable.auto.commit", "false")
        .set("auto.offset.reset", "earliest")
        .set("max.poll.interval.ms", KAFKA_MAX_POLL_INTERVAL_MS);
    builder.create()
}

fn create_producer(config: &WorkerArgs) -> Result<FutureProducer, rdkafka::error::KafkaError> {
    base_client_config(config).create()
}

fn base_client_config(config: &WorkerArgs) -> ClientConfig {
    let mut builder = ClientConfig::new();
    builder.set("bootstrap.servers", &config.brokers);
    if config.tls_enabled() {
        builder.set("security.protocol", "ssl");
        if let Some(ca) = config.tls_ca.as_ref().and_then(|p| p.to_str()) {
            builder.set("ssl.ca.location", ca);
        }
        if let Some(cert) = config.tls_cert.as_ref().and_then(|p| p.to_str()) {
            builder.set("ssl.certificate.location", cert);
        }
        if let Some(key) = config.tls_key.as_ref().and_then(|p| p.to_str()) {
            builder.set("ssl.key.location", key);
        }
    }
    builder
}
