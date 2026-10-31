import type { AuthStatus, DeviceView } from "./useSubscriptionData";

/**
 * The write half of the account surface: the API-key link form, the device-code flow and
 * the hints that explain what a write actually did. Pure props — every handler comes from
 * `useSubscriptionData`, so this component holds no state of its own.
 */
export default function SubscriptionCredentialCard(props: {
  auth: AuthStatus | null;
  apiKey: string;
  setKey: (value: string) => void;
  envShadow: boolean;
  linkMsg: string | null;
  device: DeviceView | null;
  deviceMsg: string | null;
  onLink: () => void;
  onClear: () => void;
  onStartDevice: () => void;
}) {
  const { auth, apiKey, setKey, envShadow, linkMsg, device, deviceMsg } = props;
  return (
    <>
      <div className="aj-sub__link">
        <span className="aj-sub__section">链接方式</span>
        <label className="aj-sub__link-label" htmlFor="aj-sub-api-key">
          API Key
        </label>
        <div className="aj-sub__link-row">
          <input
            id="aj-sub-api-key"
            className="aj-sub__input"
            type="password"
            autoComplete="off"
            value={apiKey}
            placeholder="sk-…"
            onChange={(e) => setKey(e.target.value)}
          />
          <button
            type="button"
            className="aj-sub__btn"
            disabled={apiKey.trim().length === 0}
            onClick={props.onLink}
          >
            链接
          </button>
          <button type="button" className="aj-sub__btn" onClick={props.onClear}>
            解除链接
          </button>
        </div>
        <p className="aj-sub__hint">
          API Key 只保存在所连接的内核主机（auth.json），不会写入浏览器存储。
        </p>
        {auth?.credential?.kind === "oauth" && (
          <p className="aj-sub__error">将替换现有 OAuth 凭据</p>
        )}
        {envShadow && (
          <p className="aj-sub__msg">已保存；环境变量当前优先，链接的密钥暂不生效</p>
        )}
        {linkMsg && <p className="aj-sub__msg">{linkMsg}</p>}
      </div>
      <div className="aj-sub__actions">
        <button type="button" className="aj-sub__btn" onClick={props.onStartDevice}>
          设备码登录
        </button>
      </div>
      {device && (
        <div className="aj-sub__device">
          <p>
            请在浏览器打开 <a href={device.verification_uri}>{device.verification_uri}</a>
          </p>
          <p>
            并输入设备码 <span className="aj-sub__code">{device.user_code}</span>
          </p>
        </div>
      )}
      {deviceMsg && <p className="aj-sub__msg">{deviceMsg}</p>}
    </>
  );
}
