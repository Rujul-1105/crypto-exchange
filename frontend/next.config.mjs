/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  experimental: {
    // App Router already enabled by default in Next 14
  },
  webpack: (config) => {
    // Some Solana wallet packages use Node-only modules; allow them in the client bundle.
    config.resolve.fallback = {
      ...config.resolve.fallback,
      fs: false,
      net: false,
      tls: false,
    };
    // Silences the "Critical dependency" warning that webpack emits for
    // viem's ox/tempo/internal/virtualMasterPool.js — it uses a dynamic
    // require() to lazy-load chain configs that webpack can't analyze.
    // Pulled in transitively via @walletconnect/solana-adapter. Harmless
    // at runtime, just noisy during dev compilation.
    config.module.parser.javascript.exprContextCritical = false;
    return config;
  },
};

export default nextConfig;
