interface ImportMetaEnv {
  CLOUDFLARE_API_KEY?: string
  CLOUDFLARE_ACCOUNT_ID?: string
  CLOUDFLARE_GATEWAY_ID?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
