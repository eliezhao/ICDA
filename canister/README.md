# Todo:

- 声明stable memory的最大值为一个量【这个量通过1周内存储总量 / Canister 个数得出】
- 在 X（存储总量计算得出） 个子网上创建canister

- 先实现mvp，然后再优化结构

1. [ ] save_blob
    2. key： blob_id string (serde_json::to_string())
    3. value: blob
    4. 根据timestamp覆盖数据，这样可以rr [todo]
2. [ ] get_blob
    3. (key: blob_id: string => value: Result<Vec<u8>>)
        1. 存储数据结构用 BTreeMap，暂定[Vec<u8> => Vec<u8>，可以做一个处理，最多存储2048个Key（4G / 2M ）, 64kb]
            1. 不过这里有个问题，update call 的 envelop 不能大于2M，所以要分2次Upload，目前还是这样吗?*【暂时先直接put，后面看情况】
               *[待确定-faraz，区块2M，要小一些]**
                1. 区块 > 2M ? or < **[faraz-待确定]**
                2. 2M / s 的吞吐量子网未必可以达到[**待确定]**
                    1. merkle proof具体怎么构造和验证[待确定-paul]
                3. 12 block ⇒ 1次切换子网
        2. Func(Key) ⇒ Canister【server最好没有状态】
3. get_signature
    4. 对什么进行sign？
    5. 目前先实现对threshold signature的调用，后面canister函数调用这个函数即可
6. todo:
    7. 加上#[query] 和 update宏
    8. btreemap在replace的时候是如何replace的？是replace memory还是?
    9. 测试还没写
        10. 内部单元测试
        11. 主网测试 + 脚本