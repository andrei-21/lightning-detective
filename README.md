# 🕵️ Lightning Detective

## 📖 About

Lightning Detective is a Rust library and a service designed to deduce the
recipient of a lightning payment.
By looking at the details of the provided BOLT-11 lightning invoice and
leveraging some knowledge of the lightning network graph.
Lightning Detective identifies whether the payee is a user of a non-custodial wallet, custodial exchange, or something else.

## 🐳 Running with Docker

Build and start the service with Docker Compose:

```sh
docker compose up --build -d
```

The service is available at <http://127.0.0.1:3000>. The Compose configuration
intentionally publishes port 3000 only on the host's loopback interface. For
remote access, prefer a TLS-enabled reverse proxy that forwards to this address.
To expose the service directly, change the host address in `compose.yaml` from
`127.0.0.1` to `0.0.0.0` and protect the port with an appropriate firewall.

## 🔧 How It Works
A lightning invoice is a set of payment instructions which has the destination
as a public key of the recipient node.
If it is a well known node like [WalletOfSatoshi.com](https://mempool.space/lightning/node/035e4ff418fc8b5554c5d9eea66396c227bd429a3251c8cbc711002ba215bfc226)
it is reasonable to infer that the recipient is a user of the custodial **Wallet of Satoshi**.

In more complex scenarios, especially prevalent in mobile wallets where
the recipient node has no announced channels nor a reachable address,
lightning invoices incorporate additional routing details,
such as the LSP node public key.
When the LSP is associated with a well known node like [ACINQ](https://mempool.space/lightning/node/03864ef025fde8fb587d989186ce6a4a186895ee44a926bfc370e2c366597a3f8f),
it is reasonable to conclude that the recipient is utilizing the non-custodial **Phoenix** wallet.
