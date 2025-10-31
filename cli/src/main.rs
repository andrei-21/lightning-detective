#![warn(unused_crate_dependencies)]

use anyhow::{anyhow, Result};
use colored::{ColoredString, Colorize};
use detective::decoder::{decode, Bip21Param, DecodedData};
use detective::offer_details::{IntroductionNode, OfferDetails};
use detective::{resolve_bip353, resolve_lnurl, Description, InvoiceDetails};
use detective::{InvestigativeFindings, InvoiceDetective, Node, RecipientNode, ServiceKind};
use std::env;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    let input = env::args().nth(1).ok_or(anyhow!("Input is required"))?;
    let decoded_data = decode(&input)?;

    let invoice_detective = InvoiceDetective::new()?;

    match decoded_data {
        DecodedData::Invoice(invoice) => {
            let invoice_details = InvoiceDetails::from(&invoice);
            print_invoice_details(invoice_details);
            let findings = invoice_detective.investigate_bolt11(&invoice)?;
            print_findings(findings)
        }
        DecodedData::Offer(offer) => {
            let offer_details = OfferDetails::from(offer.clone());
            print_offer_details(offer_details);
            let findings = invoice_detective.investigate_bolt12(offer)?;
            print_findings(findings)
        }
        DecodedData::Refund(refund) => {
            println!("{refund:?}")
        }
        DecodedData::LnUrl(lnurl) => {
            println!("{lnurl:?}");
            let mut events = resolve_lnurl(lnurl);
            while let Some(event) = events.next().await {
                println!("{event:?}");
            }
        }
        DecodedData::LightningAddress((username, domain), lnurl) => {
            println!("Lightning address of {username} at {domain}");
            let mut events = resolve_lnurl(lnurl);
            while let Some(event) = events.next().await {
                println!("{event:?}");
            }
        }
        DecodedData::Bip21(address, params) => {
            print_bip21(address, params);
        }
        DecodedData::Bip353(name) => {
            let result = resolve_bip353(&name).await?;
            println!("DNS resolves to: {}", result.bip21);
            println!("          proof: {}", result.proof);
        }
    };
    Ok(())
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
    println!("      Features: {}", "todo".red().bold());
    if !invoice.fallback_addresses.is_empty() {
        println!("     Fallbacks: {}", "todo".red().bold());
        println!("                {}", " deprecated ".reversed().yellow());
    }
    println!();
}

fn print_bip21(address: Option<String>, mut params: Vec<Bip21Param>) {
    println!("📋 {}", " BIP 21 ".reversed());

    println!("On-chain address: {}", format_option(&address));
    params.sort();
    for param in params {
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
