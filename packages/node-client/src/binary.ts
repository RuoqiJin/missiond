/**
 * Binary path resolution for missiond
 *
 * Searches for the missiond binary in the following order:
 * 1. node_modules/@missiond/core-{platform}-{arch}/bin/missiond
 * 2. PATH environment variable
 * 3. ~/.cargo/bin/missiond
 */

import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { execSync } from "child_process";

/**
 * Platform identifiers matching the npm package naming convention
 */
type Platform = "darwin" | "linux" | "win32";
type Arch = "x64" | "arm64";

/**
 * Detect libc type on Linux
 */
function detectLibc(): "gnu" | "musl" {
  // Check for musl libc
  try {
    const lddOutput = execSync("ldd --version 2>&1 || true", {
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"],
    });
    if (lddOutput.toLowerCase().includes("musl")) {
      return "musl";
    }
  } catch {
    // Ignore
  }

  // Check /lib for musl
  try {
    if (fs.existsSync("/lib/ld-musl-x86_64.so.1")) {
      return "musl";
    }
  } catch {
    // Ignore
  }

  // Default to GNU libc
  return "gnu";
}

/**
 * Get all possible package names for the current platform
 *
 * Returns an array of package names to try, in priority order.
 * For Linux, tries both GNU and MUSL variants.
 */
function getPlatformPackageNames(): string[] {
  const platform = os.platform() as Platform;
  const arch = os.arch() as Arch;

  // Map Node.js platform to package naming
  const platformMap: Record<Platform, string> = {
    darwin: "darwin",
    linux: "linux",
    win32: "win32",
  };

  const archMap: Record<Arch, string> = {
    x64: "x64",
    arm64: "arm64",
  };

  const platformId = platformMap[platform] ?? platform;
  const archId = archMap[arch] ?? arch;

  // For Linux, we need to try both GNU and MUSL
  if (platform === "linux") {
    const libc = detectLibc();
    if (libc === "musl") {
      // Try musl first, then gnu as fallback
      return [
        `core-${platformId}-${archId}-musl`,
        `core-${platformId}-${archId}-gnu`,
      ];
    }
    // Try gnu first, then musl as fallback
    return [
      `core-${platformId}-${archId}-gnu`,
      `core-${platformId}-${archId}-musl`,
    ];
  }

  // For Darwin and Windows, just platform-arch
  return [`core-${platformId}-${archId}`];
}

/**
 * Get the primary platform identifier for package naming
 *
 * @deprecated Use getPlatformPackageNames() for better Linux support
 */
function getPlatformId(): string {
  const packageNames = getPlatformPackageNames();
  return packageNames[0]!;
}

/**
 * Get the binary name for the current platform
 */
function getBinaryName(): string {
  return os.platform() === "win32" ? "missiond.exe" : "missiond";
}

/**
 * Search for binary in node_modules
 *
 * Looks for @missiond/core-{platform}-{arch}[-libc]/bin/missiond
 * Tries multiple package names for Linux to support both GNU and MUSL libc.
 */
function findInNodeModules(): string | null {
  const packageVariants = getPlatformPackageNames();
  const binaryName = getBinaryName();

  // Try each package variant
  for (const variant of packageVariants) {
    const packageName = `@missiond/${variant}`;

    // Try to find in various possible node_modules locations
    const searchPaths = [
      // Current working directory
      path.join(process.cwd(), "node_modules", packageName, "bin", binaryName),
      // This package's node_modules (when installed as dependency)
      path.join(__dirname, "..", "node_modules", packageName, "bin", binaryName),
      // Parent node_modules (hoisted)
      path.join(__dirname, "..", "..", packageName, "bin", binaryName),
      // Root node_modules
      path.join(__dirname, "..", "..", "..", packageName, "bin", binaryName),
      // Even deeper hoisting (monorepo scenarios)
      path.join(__dirname, "..", "..", "..", "..", packageName, "bin", binaryName),
    ];

    for (const searchPath of searchPaths) {
      if (fs.existsSync(searchPath) && isExecutable(searchPath)) {
        return searchPath;
      }
    }
  }

  return null;
}

/**
 * Search for binary in PATH
 */
function findInPath(): string | null {
  const binaryName = getBinaryName();

  try {
    // Use 'which' on Unix, 'where' on Windows
    const cmd = os.platform() === "win32" ? `where ${binaryName}` : `which ${binaryName}`;
    const result = execSync(cmd, { encoding: "utf-8", stdio: ["pipe", "pipe", "pipe"] }).trim();

    // 'where' on Windows may return multiple paths, take the first
    const firstPath = result.split("\n")[0]?.trim();

    if (firstPath && fs.existsSync(firstPath)) {
      return firstPath;
    }
  } catch {
    // Command failed, binary not in PATH
  }

  return null;
}

/**
 * Search for binary in Cargo bin directory
 */
function findInCargoBin(): string | null {
  const binaryName = getBinaryName();
  const cargoHome = process.env.CARGO_HOME ?? path.join(os.homedir(), ".cargo");
  const cargoBinPath = path.join(cargoHome, "bin", binaryName);

  if (fs.existsSync(cargoBinPath) && isExecutable(cargoBinPath)) {
    return cargoBinPath;
  }

  return null;
}

/**
 * Check if a file is executable
 */
function isExecutable(filePath: string): boolean {
  try {
    // On Windows, just check if file exists
    if (os.platform() === "win32") {
      return fs.existsSync(filePath);
    }

    // On Unix, check execute permission
    fs.accessSync(filePath, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/**
 * Get the path to the missiond binary
 *
 * Searches in the following order:
 * 1. node_modules/@missiond/core-{platform}-{arch}/bin/missiond
 * 2. PATH environment variable
 * 3. ~/.cargo/bin/missiond
 *
 * @returns The absolute path to the binary, or null if not found
 */
export function getBinaryPath(): string | null {
  // 1. Check node_modules (optionalDependencies)
  const nodeModulesPath = findInNodeModules();
  if (nodeModulesPath) {
    return nodeModulesPath;
  }

  // 2. Check PATH
  const pathBinary = findInPath();
  if (pathBinary) {
    return pathBinary;
  }

  // 3. Check cargo bin
  const cargoBinary = findInCargoBin();
  if (cargoBinary) {
    return cargoBinary;
  }

  return null;
}

/**
 * Get the expected package name for the current platform
 *
 * Returns the primary expected package name.
 * For Linux, this considers the detected libc type.
 */
export function getExpectedPackageName(): string {
  const variants = getPlatformPackageNames();
  return `@missiond/${variants[0]}`;
}

/**
 * Get all possible package names for the current platform
 *
 * Returns all package variants that might work on this platform.
 * For Linux, returns both GNU and MUSL variants.
 */
export function getAllExpectedPackageNames(): string[] {
  return getPlatformPackageNames().map((v) => `@missiond/${v}`);
}

/**
 * Check if the binary is available
 */
export function isBinaryAvailable(): boolean {
  return getBinaryPath() !== null;
}

/**
 * Get diagnostic information about binary search
 */
export function getBinaryDiagnostics(): {
  found: boolean;
  path: string | null;
  expectedPackages: string[];
  platform: string;
  arch: string;
  searchLocations: { location: string; exists: boolean }[];
} {
  const packageVariants = getPlatformPackageNames();
  const binaryName = getBinaryName();
  const cargoHome = process.env.CARGO_HOME ?? path.join(os.homedir(), ".cargo");

  const searchLocations: string[] = [];

  // Add node_modules locations for each package variant
  for (const variant of packageVariants) {
    const packageName = `@missiond/${variant}`;
    searchLocations.push(
      path.join(process.cwd(), "node_modules", packageName, "bin", binaryName)
    );
  }

  // Add cargo and PATH
  searchLocations.push(path.join(cargoHome, "bin", binaryName));
  searchLocations.push("PATH");

  const foundPath = getBinaryPath();

  return {
    found: foundPath !== null,
    path: foundPath,
    expectedPackages: packageVariants.map((v) => `@missiond/${v}`),
    platform: os.platform(),
    arch: os.arch(),
    searchLocations: searchLocations.map((loc) => ({
      location: loc,
      exists:
        loc === "PATH" ? findInPath() !== null : fs.existsSync(loc) && isExecutable(loc),
    })),
  };
}
