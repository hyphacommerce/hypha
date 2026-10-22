// @hypha/app-bridge-host: the admin's side of the app bridge.

export {
  attachBridge,
  BridgeHostError,
  type AttachOptions,
  type BridgeFrame,
  type HostWindow,
} from "./host.ts";
export {
  PROTOCOL_VERSION,
  type ResourceType,
  type ToastTone,
  type BridgeErrorCode,
} from "@hypha/app-bridge/protocol";
