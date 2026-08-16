# Multiplayer E2E Report（待用户实测填写）

> 模板依据专项规范 §67。所有字段必须来自真实测试，禁止估算或编造。
> 填写完成后，把 `docs/V081_MULTIPLAYER_CHECKLIST.md` 第六节对应项勾选为 `[x]`。

## Test Type

- [ ] SAME_MACHINE_PUBLIC_TUNNEL
- [ ] TWO_PHYSICAL_DEVICES
- [ ] DIFFERENT_NETWORKS

> 同机测试只能写 `PASS: Same-machine public e4mc tunnel E2E`，禁止写成跨设备/异网结论。
> 若同机公网回环在当前网络不可用：记录 `SAME_MACHINE_PUBLIC_TUNNEL_UNSUPPORTED_BY_CURRENT_NETWORK`，
> 并保持对应 Checklist 为 `[ ]`，禁止偷换 localhost/LAN 路径。

## Environment

- Host Physical Device: SAME_MACHINE
- Guest Physical Device: SAME_MACHINE
- Host Instance:
- Guest Instance:
- Connection Used: `*.e4mc.link`
- LAN/localhost used: NO
- Host network:
- Guest network:
- Minecraft:
- Loader:
- e4mc:
- SH Launcher:

## Host Public Tunnel

- session_id:
- instance_id:
- Minecraft version:
- Loader:
- e4mc version:
- LAN port:
- public endpoint:
- time_to_ready:

## Create

- Prepare:
- Game start:
- LAN open:
- Public endpoint:
- Time to ready:

## Join

- Address:
- Join result:
- Join latency:
- Authentication/session result:

## Stability

- Duration:
- Disconnects:
- Reconnects:
- Gameplay issues:

## Exit

- Host exit:
- Guest behavior:
- SH state:
- Watcher cleanup:
- Process cleanup:

## Recreate

- Second room:
- Third room:

## Result

PASS / FAIL

> 备注：成功样本少时只能写“本次测试 N/N 成功”，不得外推百分比。
