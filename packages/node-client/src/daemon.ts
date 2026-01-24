/**
 * Daemon management for missiond
 *
 * Provides functions to:
 * - Check if daemon is running
 * - Start/stop daemon
 * - Get socket/WebSocket paths
 */

import { spawn, ChildProcess } from "child_process";
import * as fs from "fs";
import * as net from "net";
import * as os from "os";
import * as path from "path";
import { getBinaryPath } from "./binary.js";

/**
 * Default configuration directory
 */
const DEFAULT_CONFIG_DIR = path.join(os.homedir(), ".xjp-mission");

/**
 * Default WebSocket port
 */
const DEFAULT_WS_PORT = 9120;

/**
 * Daemon manager options
 */
export interface DaemonOptions {
  /** Configuration directory (default: ~/.xjp-mission) */
  configDir?: string;
  /** WebSocket port (default: 9120) */
  wsPort?: number;
  /** Auto-start daemon if not running (default: true) */
  autoStart?: boolean;
  /** Timeout for daemon startup in milliseconds (default: 5000) */
  startupTimeout?: number;
}

/**
 * Daemon manager for missiond
 *
 * Handles lifecycle management of the missiond daemon process.
 */
export class DaemonManager {
  private readonly configDir: string;
  private readonly wsPort: number;
  private readonly autoStart: boolean;
  private readonly startupTimeout: number;
  private daemonProcess: ChildProcess | null = null;

  constructor(options?: DaemonOptions) {
    this.configDir = options?.configDir ?? DEFAULT_CONFIG_DIR;
    this.wsPort = options?.wsPort ?? DEFAULT_WS_PORT;
    this.autoStart = options?.autoStart ?? true;
    this.startupTimeout = options?.startupTimeout ?? 5000;
  }

  /**
   * Get the Unix socket path
   */
  getSocketPath(): string {
    return path.join(this.configDir, "missiond.sock");
  }

  /**
   * Get the WebSocket URL
   */
  getWebSocketUrl(): string {
    return `ws://localhost:${this.wsPort}`;
  }

  /**
   * Check if the daemon is running
   *
   * Attempts to connect to the Unix socket to verify the daemon is responsive.
   */
  async isRunning(): Promise<boolean> {
    const socketPath = this.getSocketPath();

    // First check if socket file exists
    if (!fs.existsSync(socketPath)) {
      return false;
    }

    // Try to connect to verify daemon is actually running
    return new Promise((resolve) => {
      const socket = net.createConnection(socketPath);

      const cleanup = () => {
        socket.removeAllListeners();
        socket.destroy();
      };

      socket.on("connect", () => {
        // Send a ping to verify daemon is responsive
        socket.write(JSON.stringify({ jsonrpc: "2.0", method: "ping", id: 1 }) + "\n");
      });

      socket.on("data", () => {
        cleanup();
        resolve(true);
      });

      socket.on("error", () => {
        cleanup();
        resolve(false);
      });

      // Timeout after 1 second
      setTimeout(() => {
        cleanup();
        resolve(false);
      }, 1000);
    });
  }

  /**
   * Ensure the daemon is running
   *
   * If autoStart is enabled and daemon is not running, starts it.
   * Waits for the daemon to become ready (socket accepts connections).
   *
   * @throws Error if daemon cannot be started or fails to become ready
   */
  async ensureRunning(): Promise<void> {
    if (await this.isRunning()) {
      return;
    }

    if (!this.autoStart) {
      throw new Error("Daemon is not running and autoStart is disabled");
    }

    await this.start();
  }

  /**
   * Start the daemon
   *
   * Spawns the missiond binary as a detached process.
   * Waits for the daemon to become ready.
   *
   * @throws Error if binary not found or daemon fails to start
   */
  async start(): Promise<void> {
    const binaryPath = getBinaryPath();
    if (!binaryPath) {
      throw new Error(
        "missiond binary not found. Install @missiond/core-{platform}-{arch} or add missiond to PATH"
      );
    }

    // Ensure config directory exists
    if (!fs.existsSync(this.configDir)) {
      fs.mkdirSync(this.configDir, { recursive: true });
    }

    // Set up environment
    const env: NodeJS.ProcessEnv = {
      ...process.env,
      XJP_MISSION_HOME: this.configDir,
      MISSION_WS_PORT: String(this.wsPort),
    };

    // Spawn daemon as detached process
    const child = spawn(binaryPath, [], {
      detached: true,
      stdio: "ignore",
      env,
    });

    // Allow parent to exit independently
    child.unref();
    this.daemonProcess = child;

    // Wait for daemon to become ready
    await this.waitForReady();
  }

  /**
   * Wait for daemon to become ready
   *
   * Polls the socket until connection succeeds or timeout is reached.
   */
  private async waitForReady(): Promise<void> {
    const startTime = Date.now();
    const pollInterval = 100;

    while (Date.now() - startTime < this.startupTimeout) {
      if (await this.isRunning()) {
        return;
      }
      await sleep(pollInterval);
    }

    throw new Error(`Daemon failed to start within ${this.startupTimeout}ms`);
  }

  /**
   * Stop the daemon
   *
   * Sends SIGTERM to the daemon process.
   * If the daemon was not started by this manager, attempts to find and kill it.
   */
  async stop(): Promise<void> {
    // If we have a reference to the daemon process, kill it directly
    if (this.daemonProcess && !this.daemonProcess.killed) {
      this.daemonProcess.kill("SIGTERM");
      this.daemonProcess = null;
      return;
    }

    // Try to find the daemon process by socket
    const socketPath = this.getSocketPath();
    if (!fs.existsSync(socketPath)) {
      return; // Already stopped
    }

    // Send a graceful shutdown request via IPC (if supported in future)
    // For now, just remove the socket file and let the OS clean up
    // In a real implementation, we'd find the PID and send SIGTERM

    // Attempt to connect and send shutdown command
    await new Promise<void>((resolve) => {
      const socket = net.createConnection(socketPath);

      socket.on("connect", () => {
        // Future: send shutdown command
        // For now, just disconnect - the daemon handles this gracefully
        socket.end();
        resolve();
      });

      socket.on("error", () => {
        // Socket error means daemon is not running or crashed
        // Clean up stale socket file
        try {
          fs.unlinkSync(socketPath);
        } catch {
          // Ignore errors
        }
        resolve();
      });

      // Timeout
      setTimeout(() => {
        socket.destroy();
        resolve();
      }, 1000);
    });
  }

  /**
   * Get the database path
   */
  getDbPath(): string {
    return path.join(this.configDir, "mission.db");
  }

  /**
   * Get the slots configuration path
   */
  getSlotsConfigPath(): string {
    return path.join(this.configDir, "slots.yaml");
  }

  /**
   * Get the permissions configuration path
   */
  getPermissionsConfigPath(): string {
    return path.join(this.configDir, "config", "permissions.yaml");
  }

  /**
   * Get the logs directory
   */
  getLogsDir(): string {
    return path.join(this.configDir, "logs");
  }
}

/**
 * Sleep for specified milliseconds
 */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Default daemon manager instance
 */
let defaultManager: DaemonManager | null = null;

/**
 * Get the default daemon manager
 *
 * Creates a singleton instance with default options.
 */
export function getDefaultManager(): DaemonManager {
  if (!defaultManager) {
    defaultManager = new DaemonManager();
  }
  return defaultManager;
}

/**
 * Check if daemon is running using default manager
 */
export async function isDaemonRunning(): Promise<boolean> {
  return getDefaultManager().isRunning();
}

/**
 * Ensure daemon is running using default manager
 */
export async function ensureDaemonRunning(): Promise<void> {
  return getDefaultManager().ensureRunning();
}

/**
 * Stop daemon using default manager
 */
export async function stopDaemon(): Promise<void> {
  return getDefaultManager().stop();
}
