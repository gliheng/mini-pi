# pi-commander

A web client for the [mini-pi](https://github.com/earendil-works/mini-pi) remote control API. Connects to a running mini-pi desktop instance through its Cloudflare Tunnel and lets you create threads, send messages, and stream assistant replies from any browser.

Built with [Nuxt](https://nuxt.com), [Nuxt UI](https://ui.nuxt.com), and deployed on [Cloudflare Workers](https://workers.cloudflare.com).

## Setup

Install dependencies:

```bash
yarn install
```

## Development

Start the development server:

```bash
yarn dev
```

Open the app, click **Remote settings** (or the settings icon in the sidebar), and enter:

- **Tunnel URL**: the public URL shown in mini-pi (e.g. `https://abc123.trycloudflare.com`)
- **Bearer token**: the token configured in mini-pi remote control settings

The settings are stored in your browser.

## Build

```bash
yarn build
```

## Deploy

Deploy to Cloudflare Workers via NuxtHub:

```bash
yarn deploy
```

Or use Wrangler directly after building:

```bash
npx wrangler deploy
```

## Architecture

- The Nuxt UI runs entirely in the browser.
- All chat state lives in the remote mini-pi instance accessed through its Cloudflare Tunnel.
- REST calls use `Authorization: Bearer <token>`.
- Assistant replies stream from `POST /threads/:id/message` as AI SDK UI message chunks over Server-Sent Events.
- No server-side database, file uploads, or authentication are required.
