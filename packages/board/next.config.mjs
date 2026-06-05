import { PHASE_DEVELOPMENT_SERVER } from 'next/constants.js';

const standaloneOutput = process.env.MISSIOND_BOARD_STANDALONE === '1';

/** @type {import('next').NextConfig} */
const nextConfig = (phase) => {
  const isDev = phase === PHASE_DEVELOPMENT_SERVER;
  return {
    reactStrictMode: true,
    distDir: isDev ? '.next-dev' : '.next',
    ...(standaloneOutput ? { output: 'standalone' } : {}),
    async rewrites() {
      const missiondPort = process.env.NEXT_PUBLIC_WS_PORT || '9120';
      return [
        {
          source: '/missiond/:path*',
          destination: `http://localhost:${missiondPort}/:path*`,
        },
      ];
    },
  };
};

export default nextConfig;
