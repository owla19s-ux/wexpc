# API-PC-01 可运行类型提案

状态：供双方核对的std-only Rust类型提案，不是已采用API模块。Work维护；不占用鱼计划接手的`crates/wex-api`目录，不改工作区/锁文件，不预判D-002/D-004。

```sh
rustc --edition=2021 --crate-type lib -D warnings contracts/api_pc_01.rs -o /tmp/libwex_api_contract.rlib
rustc --edition=2021 --test -A dead_code contracts/tests/api_pc_01.rs -o /tmp/wex-api-contract-tests
/tmp/wex-api-contract-tests
```

手机使用自己的可写临时目录替换`/tmp`；Work可用`/root/.cargo/bin/rustc`绝对路径。测试编译只允许未被用到的提案类型产生dead_code，否则不屏蔽其他诊断；库本身以-D warnings检查。

已验证4项：取消标记的共享可见性、续传origin全字段匹配、Credential/续传payload的Debug脱敏、稳定错误码。取消只是标记，不保证网络/通道被解除；matches_origin只是纯比较，不替代应用层凭证/授权/生命周期核验。JSON为字符串载体，解析/Schema/终态校验仍由执行器实现，不把类型构造成功当作校验通过。

没有HTTP/SSE执行器、通道/join签名、默认上限、自动重试或工具执行；不是可交付产品。尚无手机/Windows实测，不冒称网络取消或完整API已跑通。下一步由鱼核对类型提案，工作区与执行方式争议明确后再迁入正式crate并接线。
