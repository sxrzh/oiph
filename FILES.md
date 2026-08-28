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
            - data.yaml
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