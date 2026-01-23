#![warn(unused_crate_dependencies)]

use anyhow::{anyhow, Result};
use colored::{ColoredString, Colorize};
use detective::decoder::{decode, Bip21, Bip21Param, DecodedData};
use detective::offer_details::{IntroductionNode, OfferDetails};
use detective::{
    resolve_bip353, resolve_lnurl, Description, Image, InvoiceDetails, JsonRpcEvent, LnUrlResponse,
    PayOfferParams,
};
use detective::{InvestigativeFindings, InvoiceDetective, Node, RecipientNode, ServiceKind};
use std::env;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .format_timestamp_millis()
        .init();

    let input = env::args().nth(1).ok_or(anyhow!("Input is required"))?;
    let decoded_data = decode(&input)?;

    let invoice_detective = InvoiceDetective::new()?;

    match decoded_data {
        DecodedData::OnchainAddress(address) => {
            println!("📋 {}", " On-Chain Address ".reversed());
            println!("Address: {}", address.address);
            println!("   Type: {}", format_option(&address.address_type));
            let networks = address
                .valid_networks
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            println!("Network: {networks}");
        }
        DecodedData::SilentPaymentAddress(address) => {
            println!("📋 {}", " Silent Payment Address ".reversed());
            println!("     Network: {:?}", address.get_network());
            println!(" Scan pubkey: {}", address.get_scan_key());
            println!("Spend pubkey: {}", address.get_spend_key());
        }
        DecodedData::Invoice(invoice) => {
            let invoice_details = InvoiceDetails::from(&invoice);
            print_invoice_details(invoice_details);
            let findings = invoice_detective.investigate_bolt11(&invoice)?;
            print_findings(findings)
        }
        DecodedData::Offer(offer) => {
            let offer_details = OfferDetails::from(offer.clone());
            print_offer_details(offer_details);
            let params = PayOfferParams {
                amount_msats: Some(100),
                ..Default::default()
            };
            let mut events = detective::request_bolt12_invoice(offer, params).await;
            while let Some(event) = events.next().await {
                println!("{event:?}");
            }
            // let findings = invoice_detective.investigate_bolt12(offer)?;
            // print_findings(findings)
        }
        DecodedData::Refund(refund) => {
            println!("{refund:?}")
        }
        DecodedData::LnUrl(lnurl) => {
            println!("{lnurl:?}");
            let mut events = resolve_lnurl(lnurl);
            while let Some(event) = events.next().await {
                match event {
                    JsonRpcEvent::Result(Ok(response)) => {
                        print_lnurl_details(response);
                    }
                    JsonRpcEvent::Result(Err(error)) => {
                        eprintln!("{}", format!("Error: {error}").red())
                    }
                    event => println!("{}", format!("{event:?}").dimmed()),
                }
            }
        }
        DecodedData::LightningAddress(lightning_address) => {
            println!(
                "Lightning address of {} at {}",
                lightning_address.username, lightning_address.domain
            );
            let mut events = resolve_lnurl(lightning_address.lnurl);
            while let Some(event) = events.next().await {
                println!("{event:?}");
            }
        }
        DecodedData::Bip21(bip21) => {
            print_bip21(bip21);
        }
        DecodedData::Bip353(hrn) => {
            let result = resolve_bip353(&hrn).await?;
            println!("DNS resolves to: {}", result.bip21);
            println!("          proof: {}", result.proof);
        }
        DecodedData::Bip353OrLightningAddress(hrn, lightning_address) => {
            match resolve_bip353(&hrn).await {
                Ok(result) => {
                    println!("DNS resolves to: {}", result.bip21);
                    println!("          proof: {}", result.proof);
                }
                Err(e) => println!("Not a BIP-353 DNS: {e}"),
            };
            println!();
            println!(
                "Lightning address of {} at {}",
                lightning_address.username, lightning_address.domain
            );
            let mut events = resolve_lnurl(lightning_address.lnurl);
            while let Some(event) = events.next().await {
                println!("{event:?}");
            }
        }
    };
    Ok(())
}

fn print_lnurl_details(details: LnUrlResponse) {
    println!();
    match details {
        LnUrlResponse::Pay(pay) => {
            println!("📋 {}", " LNURL Pay ".reversed());
            println!("Send amount: {}", pay.sendable_amount);
            println!("Description: {}", pay.description);
            println!("       Long: {}", format_option(&pay.long_description));
            match pay.image {
                Some(Image::Jpeg(bytes)) => println!(" JPEG image: {} bytes", bytes.len()),
                Some(Image::Png(bytes)) => println!("  PNG image: {} bytes", bytes.len()),
                None => println!("      Image: {}", "empty".italic().dimmed()),
            };
            let comment = pay.comment_allowed.map(|c| format!("Up to {c} chars"));
            println!("    Comment: {}", format_option(&comment));
            println!("   Callback: {}", pay.callback);
            for (key, value) in pay.metadata {
                println!("   Metadata: {key}: {value}");
            }
        }
        _ => todo!(),
    }
    println!();
}

fn print_offer_details(d: OfferDetails) {
    println!("📋 {}", " Details ".reversed());
    println!("         Id: {}", d.id);
    println!("Sign pubkey: {}", format_option(&d.signing_pubkey));
    println!("     Chains: {}", d.chains.join(", "));
    println!("     Amount: {}", format_option(&d.amount));
    println!("   Quantity: {}", d.supported_quantity);
    println!("Description: {}", format_option(&d.description));
    println!("     Issuer: {}", format_option(&d.issuer));
    let expires_at = d.expires_at.map(|d| d.to_rfc2822());
    println!(" Expires at: {}", format_option(&expires_at));
    println!("   Metadata: {}", format_option(&d.metadata));
    println!("   Features: {}", format_option(&d.features.hex));
    let features = d
        .features
        .features
        .iter()
        .map(|(name, flag)| format!("             {name}: {flag:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    println!("{features}");

    for (i, path) in d.paths.iter().enumerate() {
        println!(
            "   Paths #{i}: Intro {}",
            format_introduction_node(&path.introduction_node)
        );
        println!("               with blinding {}", path.blinding_point);
        for (i, hop) in path.hops.iter().enumerate() {
            println!("             Hop #{i} {}", hop.node_id);
            println!("                    with data {}", hop.encrypted_payload);
        }
    }
    println!();
}

fn print_findings(findings: InvestigativeFindings) {
    println!("🔎 {}", " Investigative findings ".reversed());
    let recipient = format_recipient_node(&findings.recipient);
    println!("   Recipient: {recipient}");

    println!();
    println!("🗃️  {}", " Evidences ".reversed());
    println!("   Pay to {}", format_node_name(&findings.payee));
    for hint in findings.route_hints {
        let hint = hint
            .iter()
            .map(format_node_name)
            .collect::<Vec<_>>()
            .join(" → ");
        println!("     via {hint}");
    }
    if let Some(botlz_mrh_pubkey) = findings.botlz_mrh_pubkey {
        println!("   Boltz magic routing hint pubkey: {botlz_mrh_pubkey}");
    }
}

fn print_invoice_details(invoice: InvoiceDetails) {
    println!();
    println!("📋 {}", " Details ".reversed());
    println!("       Network: {}", invoice.network);
    println!("        Amount: {}", format_option(&invoice.amount));
    print!("   Description: ");
    match invoice.description {
        Description::Direct(description) if description.is_empty() => {
            println!("{}", "empty".italic().dimmed())
        }
        Description::Direct(description) => println!("{description}"),
        Description::Hash(hash) => {
            println!("{hash}");
            println!(
                "                {}",
                "description hash was provided".dimmed().italic()
            );
        }
    };
    println!("    Created at: {}", invoice.created_at);
    let has_expired = if invoice.has_expired {
        " expired ".reversed().yellow()
    } else {
        "".into()
    };
    println!("   Expiry time: {} {has_expired}", invoice.expiry);
    println!("  Payment hash: {}", invoice.payment_hash);
    println!("Payment secret: {}", invoice.payment_secret);
    println!("  Payee pubkey: {}", invoice.payee_pub_key);
    if invoice.payee_pub_key_recovered {
        println!(
            "                {}",
            "the pubkey was recovered from the invoice signature"
                .dimmed()
                .italic()
        );
    }
    println!(
        "      Metadata: {}",
        format_option(&invoice.payment_metadata)
    );
    println!("Min final CLTV: {}", invoice.min_final_cltv_expiry_delta);
    if invoice.route_hints.is_empty() {
        println!("        Routes: {}", "none".dimmed().italic());
    } else {
        println!("        Routes: {}", "todo".red().bold());
    }
    println!("      Features: {}", format_option(&invoice.features.hex));
    let features = invoice
        .features
        .features
        .iter()
        .map(|(name, flag)| format!("             {name}: {flag:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    println!("{features}");
    if !invoice.fallback_addresses.is_empty() {
        println!("     Fallbacks: {}", "todo".red().bold());
        println!("                {}", " deprecated ".reversed().yellow());
    }
    println!();
}

fn print_bip21(bip21: Bip21) {
    println!("📋 {}", " BIP 21 ".reversed());

    let address = bip21.address.map(|a| a.address);
    println!("On-chain address: {}", format_option(&address));
    //    params.sort();
    for param in bip21.params {
        match param {
            Bip21Param::Amount(amount) => println!("          Amount: {amount}"),
            Bip21Param::Label(v) => println!("           Label: {v}"),
            Bip21Param::Message(v) => println!("         Message: {v}"),
            Bip21Param::Lightning(v) => println!(" BOLT 11 invoice: {v}"),
            Bip21Param::Offer(v) => println!("   BOLT 12 offer: {v}"),
            Bip21Param::SilentPayment(v) => println!("  Silent Payment: {v}"),
            Bip21Param::PayjoinEndpoint(v) => println!("Payjoin Endpoint: {v}"),
            Bip21Param::PayjoinDisallowOutputSubstitution => {
                println!("Payjoin Disallow Output Substitution")
            }
            Bip21Param::Unknown(key, value) => println!("Unknown param {key}={value}"),
        }
    }
}

fn format_option<T: ToString>(value: &Option<T>) -> ColoredString {
    match value {
        Some(value) => value.to_string().into(),
        None => "empty".italic().dimmed(),
    }
}

fn format_introduction_node(node: &IntroductionNode) -> String {
    match node {
        IntroductionNode::NodeId(pubkey) => format!("Node {pubkey}"),
        IntroductionNode::LeftEnd(channel) => format!("Left end of {channel}"),
        IntroductionNode::RightEnd(channel) => format!("Right end of {channel}"),
    }
}

fn format_node_name(node: &Node) -> String {
    let visibility = match node.is_announced {
        true => "public",
        false => "private",
    };
    match &node.alias {
        Some(alias) => format!("{visibility} node alias:{}", alias.bold()),
        None => format!("{visibility} node id:{}", node.pubkey.bold()),
    }
}

fn format_service_kind(service: &ServiceKind) -> &str {
    match service {
        ServiceKind::BusinessWallet => "Payment processor",
        ServiceKind::ConsumerWallet => "Consumer wallet",
        ServiceKind::Exchange => "Exchange",
        ServiceKind::Lsp => "LSP",
        ServiceKind::Spark => "Spark",
    }
}

fn format_recipient_node(node: &RecipientNode) -> String {
    match node {
        RecipientNode::Custodial { custodian } => format!(
            "Custodial {} {}",
            format_service_kind(&custodian.service),
            custodian.name.bold()
        ),
        RecipientNode::NonCustodial { id, lsp } => format!(
            "Non-custodial {} {} with id:{}",
            format_service_kind(&lsp.service),
            lsp.name.bold(),
            id.bold()
        ),
        RecipientNode::NonCustodialWrapped { lsp } => {
            format!(
                "Non-custodial {} {}",
                format_service_kind(&lsp.service),
                lsp.name.bold()
            )
        }
        RecipientNode::Unknown => "Unknown".to_string(),
    }
}
