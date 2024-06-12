# Todo:

- 声明stable memory的最大值为一个量【这个量通过1周内存储总量 / Canister 个数得出】
    - 当前使用20个Canister（加上冗余就是20*2=40个）
    - 每个Canister存储60G， 那么需要(604800.0 * 2 / (60*1024)) = 20 个Canister
    - 每个Canister（60G）存储30240个Block【当前使用值】
- 在 X（存储总量计算得出） 个子网上创建canister
    - 40个Canister即20个子网
        - 20个子网目前ic有
        - 满足条件的20个子网【60G+ Stable Memory要求】
- 在dfx arg中写stable memory的配置

- [✓] save_blob
    1. key： blob_id string (serde_json::to_string())
    2. value: blob
    3. 根据timestamp覆盖数据，这样可以rr [✓]
        1. time heap [✓]
- [✓] get_blob
    1. (key: blob_id: string => value: Result<Vec<u8>>)
        1. 存储数据结构用 BTreeMap，暂定[Vec<u8> => Vec<u8>，可以做一个处理，最多存储2048个Key（4G / 2M ）, 64kb] ✓
            1. 不过这里有个问题，update call 的 envelop 不能大于2M，所以要分2次Upload，目前还是这样吗?*【暂时先直接put，后面看情况】
               *[待确定-faraz，区块2M，要小一些]**
                1. 区块 > 2M ? or < **[faraz-待确定]**
                2. 2M / s 的吞吐量子网未必可以达到[**待确定]**
                    1. merkle proof具体怎么构造和验证[待确定-paul]
- get_signature
    1. 对什么进行sign？
    2. 目前先实现对threshold signature的调用，后面canister函数调用这个函数即可 ✓
- todo:
    1. 加上#[query] 和 update宏 [✓]
    2. btreemap在replace的时候是如何replace的？是replace memory还是?
    3. 测试还没写
        1. 内部单元测试 [✓]
        2. 主网测试 + 脚本
    4. candid interface generation [✓]
    5. time_heap 和 signature batch的持久化

新版本:
digest 作为唯一的key，可以辅以index
