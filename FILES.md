## 目录结构  

contest_name
    - contest.yaml
    - problem_name_a
        - problem.yaml
        - statement
            - zh_cn.md
            - down
                - 下发文件
        - data
            - 1.in
            - 1.ans
            - ... 
        - auxiliary
            - generator.cpp
            - validator.cpp
            - checker.cpp
            - interactive_lib.cpp
    - problem_name_b
        - ...

## 配置文件结构

配置文件全部采用 YAML 格式

contest.yaml 包括比赛名称、题目列表（对应子目录名称）
problem.yaml 包括题目名称、类型时间限制、空间限制、编译选项、测试点配置
测试点配置是一个列表，每一项表示一个子任务，子任务有以下字段：
例如：
    score: 30        # 分数
    type: sum        # 子任务计分方式，包括 sum, min, mul
    pretest: true    # 是否是 pretest，不写此字段则默认为 false
    sample: true     # 是否是样例，不写此字段则默认为 false，如果是样例则导出时会自动放进 statement/down 里
    depend: []       # 一个列表，表示依赖的子任务编号（从 1 开始），如果列表中任意一个子任务不是满分则此子任务自动记为 0 分

