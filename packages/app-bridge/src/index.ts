// @hypha/app-bridge: the app's side of the admin bridge (design note 0029, §6.3).

export {
  createBridge,
  BridgeError,
  type Bridge,
  type BridgeOptions,
  type BridgeWindow,
  type HeightWatcher,
} from "./bridge.ts";
export * from "./protocol.ts";
