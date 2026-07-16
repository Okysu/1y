---
title: STM 银行转账
---

# STM 银行转账

多账户并发转账是并发编程里的经典场景:若干账户要互相扣款、加款,期间还可能余额不足、需要等待。传统方案需要为每个账户加锁,并小心地约定锁的顺序以避免死锁——一旦账户数量上升,锁的组合复杂度会爆炸。1y 用软件事务内存(STM)给出了无锁的答案:`shared` 声明可共享账户,`transact` 把多次读写打包成原子操作,`retry` 优雅地等待余额到位,多个 Actor 可以并发调用同一个转账函数而互不踩踏。本例完整展示从账户建模到不变量验证的整个流程。

## shared 账户

每个账户是一个 `shared` 单元,持有一个整数余额。`shared` 单元是版本化的可变引用,可以在事务外直接读取,也可以在事务内被原子地修改:

```1y
shared acct_alice = 1000;
shared acct_bob = 500;
shared acct_carol = 300;
shared total_transfers = 0;   // metrics: number of successful transfers

fn total_money() -> Int {
    transact {
        acct_alice + acct_bob + acct_carol
    }
}

println("initial total: " + str(total_money()));   // 1800
```

三个账户加起来是 1800,这是我们稍后要守恒的不变量。`total_transfers` 是一个度量计数器,用来统计成功的转账次数。注意 `total_money` 用 `transact` 包裹读取——这样三次读取看到的是同一时刻的一致快照,而不是"读到 alice 的旧值、再读到 bob 的新值"这种错乱状态。

为了在转账函数里方便地按名字操作账户,我们再准备两个辅助函数。它们直接访问 `shared` 单元,但在 `transact` 内部时,读取看到的是快照,写入会被缓冲到提交:

```1y
fn read_account(name) -> Int {
    match name {
        "alice" => acct_alice,
        "bob" => acct_bob,
        "carol" => acct_carol,
        _ => 0
    }
}

fn write_account(name, balance) {
    match name {
        "alice" => acct_alice = balance,
        "bob" => acct_bob = balance,
        "carol" => acct_carol = balance,
        _ => nil
    }
}
```

## 原子转账

核心的转账函数用 `transact` 包住"读源账户、读目标账户、检查余额、写两个账户、累加计数器"这几步。任何一步失败,整批改动都会回滚;只有全部成功且未被别的事务干扰时,才会一次性提交:

```1y
fn transfer(src_name, dst_name, amount) -> Bool {
    let attempts = 0;
    transact {
        attempts = attempts + 1;
        if attempts > 3 { false } else {
            let src_balance = read_account(src_name);
            let dst_balance = read_account(dst_name);

            if src_balance < amount {
                retry   // wait for funds — restart the transaction
            };

            // Write both — committed atomically.
            write_account(src_name, src_balance - amount);
            write_account(dst_name, dst_balance + amount);
            total_transfers = total_transfers + 1;
            true
        }
    }
}
```

`transact { ... }` 的尾表达式是布尔值,表示这次转账是否成功。注意这里有两个关键设计:**读和写都在事务内**,所以"检查余额—扣款—加款"不会被别的并发事务插入中间状态;**两个 `write_account` 在提交前都只是缓冲**,提交时一次性原子写入,要么都生效,要么都不生效——绝不会有"扣了款却没加款"的中间状态对外可见。

## retry 等待余额

如果源账户余额不足怎么办?直接返回 `false` 是一种选择,但 STM 给了更优雅的工具——`retry`。`retry` 会放弃当前事务并从头重跑,直到能无冲突地提交:

```1y
if src_balance < amount {
    retry   // wait for funds — restart the transaction
};
```

这里有一个陷阱:如果余额永远凑不够,事务就会无限重试。1y 用一条规则化解了它——**只有 `shared` 写入会被重试丢弃,普通的 `let` 变量在重试中保留**。所以我们用一个 `let attempts = 0` 计数器,每次重试自增,超过 3 次就主动放弃并返回 `false`:

```1y
let attempts = 0;
transact {
    attempts = attempts + 1;
    if attempts > 3 { false } else {
        // ... actual transfer logic ...
    }
}
```

`attempts` 不被回滚,所以它忠实地记录了重试次数。这是 1y STM 的"等待条件"模式:用 `retry` 等待某个共享条件被别的执行流满足,同时用 `let` 变量限制重试次数防止死循环。STM 默认最多重试 64 次,这里我们用更紧的 3 次上限,让"余额不足"尽早失败。

## Actor + STM 协作

转账逻辑是普通的 `transact` 函数,可以同时被多个 Actor 调用。我们定义一个 `Teller` actor 接收转账请求,在内部调用上面的 `transfer`:

```1y
actor Teller {
    on transfer(src, dst, amount) {
        let ok = transfer(src, dst, amount);
        reply(ok)
    }
    on balance(name) {
        reply(read_account(name))
    }
}

let teller = spawn Teller();
```

多个 `Teller` 可以并行运行,各自处理用户的转账请求。STM 保证它们对 `shared` 账户的修改互不干扰:若两个事务冲突,运行时自动重试其中一个,代码里不需要任何锁:

```1y
// Alice sends 100 to Bob.
let r1 = teller ? transfer("alice", "bob", 100);
println("alice -> bob 100: " + str(r1));              // true

// Bob sends 50 to Carol.
let r2 = teller ? transfer("bob", "carol", 50);
println("bob -> carol 50: " + str(r2));               // true

// Carol tries to send 99999 (insufficient funds — gives up after retries).
let r3 = teller ? transfer("carol", "alice", 99999);
println("carol -> alice 99999: " + str(r3));          // false
```

第三笔因为 Carol 余额不足,重试 3 次后返回 `false`。注意代码读起来就像单线程顺序执行——`teller ? transfer(...)` 同步等待结果——但底层多个请求可以并发跑,而账户的完整性由 STM 守护。

## 回滚演示

事务的原子性不仅保护账户余额,也保护事务内对任何 `shared` 单元的写入——包括 `audit_log` 这种集合。下面的 `audited_transfer` 在转账同时往日志里追加一条记录;如果转账金额过大触发 `raise`,所有改动(账户余额、日志追加、计数器)都会一起回滚:

```1y
shared audit_log = [];

fn audited_transfer(src, dst, amount) -> Bool {
    try {
        transact {
            let s = read_account(src);
            let d = read_account(dst);
            write_account(src, s - amount);
            write_account(dst, d + amount);
            audit_log = push(audit_log, src + "->" + dst + " " + str(amount));
            if amount > 400 { raise "blocked: amount too large" };
            total_transfers = total_transfers + 1;
            true
        }
    } rescue as _e {
        false
    }
}

let r4 = audited_transfer("alice", "carol", 500);
println("audited alice -> carol 500: " + str(r4));    // false (blocked)
println("audit_log entries: " + str(count(audit_log)));  // 0 — rolled back
```

这里发生了几件事:

- 事务先扣了 Alice、加了 Carol、往 `audit_log` 追加了一条记录——但这些都是**缓冲写入**,尚未提交。
- `if amount > 400 { raise ... }` 抛出异常,整个 `transact` 的写集被丢弃。
- `try`/`rescue` 捕获异常,函数返回 `false`。
- 最终 `count(audit_log)` 是 `0`——`push` 的那次追加也被回滚了。

这体现了 `shared` 集合的 COW(Copy-on-Write)语义:事务内的 `push(audit_log, ...)` 没有原地把元素塞进列表,而是返回了一个新的列表版本,只有事务提交时才会切换到这个新版本。回滚时只要丢弃新版本即可,日志保持原样。

## 不变量验证

整个示例跑完之后,我们检查最终状态:

```1y
println("");
println("=== Final state ===");
println("alice:  " + str(acct_alice));     // 1000 - 100 = 900
println("bob:    " + str(acct_bob));       // 500 + 100 - 50 = 550
println("carol:  " + str(acct_carol));     // 300 + 50 = 350
println("total:  " + str(total_money()));  // 1800 (conserved)
println("successful transfers: " + str(total_transfers));  // 2
```

关键不变量是**总额守恒**:无论发生多少笔成功的转账,所有账户的余额之和始终是 1800。两笔成功的转账(Alice→Bob 100,Bob→Carol 50)只是把钱在账户之间挪动,总额不变;第三笔 Carol→Alice 因余额不足失败,没动过分毫;第四笔被 `raise` 阻断,事务回滚,也没动过分毫。最终 `total_money()` 仍是 1800,这正是 STM 原子性给出的保证——每一笔事务要么整体生效要么整体回滚,绝不会出现"扣了款却没加款"这种破坏守恒的中间状态。

## 与锁方案对比

同样的银行转账,如果用传统锁来写,得为每个账户配一把锁,并约定加锁顺序以防死锁。STM 方案与之对照如下:

| 维度 | 传统锁方案 | 1y STM 方案 |
|------|------------|-------------|
| 死锁 | 必须小心约定全局锁顺序,否则两个转账互相等对方释放锁就死锁 | 完全没有锁,运行时检测冲突自动重试,无死锁 |
| 组合性 | 新增一个涉及的账户就要重新设计锁顺序,组合性差 | 把新账户加进 `transact` 即可,事务可自由组合 |
| 可读性 | 代码被锁的获取/释放打断,与业务逻辑纠缠 | 代码读起来像单线程顺序逻辑,锁的存在感为零 |
| 失败处理 | 死锁/超时要自己处理,回滚状态需手动恢复 | 冲突自动重试,失败自动回滚到事务开始前的状态 |
| 余额不足 | 需要条件变量、`wait`/`notify`,容易写错 | 一个 `retry` 搞定,语义即"重试到条件成立" |
| 性能 | 锁粒度粗则并发度低,细则容易死锁 | 乐观并发,冲突少时几乎零开销 |

核心洞见:**没有锁、没有锁顺序、没有死锁**。如果两个事务冲突,STM 运行时自动重试其中一个。代码读起来像单线程顺序逻辑,在并发下却是安全的。这正是 STM 的价值所在——把"安全的并发"从一件需要精心设计锁协议的难事,降维成"把相关写操作放进同一个 `transact`"这件直白的事。
