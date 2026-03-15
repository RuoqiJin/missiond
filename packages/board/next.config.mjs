/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  output: 'standalone',
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

export default nextConfig;
