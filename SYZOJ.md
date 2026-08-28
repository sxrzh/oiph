# SYZOJ 帮助

评测
C++ 使用 clang++ 或 g++ 编译，命令形如  clang++/g++ source_file.cpp -o exec_file -O2 -lm -DONLINE_JUDGE -mx32 -std=c++03（对于 C++ 11 和 C++ 17，替换命令中的 -std= 参数）；
C 使用 clang 编译，命令为  clang source_file.c -o exec_file -O2 -lm -DONLINE_JUDGE -mx32；
Pascal 使用 fpc 编译，命令为  fpc source_file.pas -O2。
Java 编译时会自动检测您的代码中 public class，如果您的代码中没有 public class，请将入口类命名为 Main。
C# 使用 Mono 平台的编译器与运行环境。

请根据题目中的说明选择使用标准输入输出或文件输入输出。

个人资料
本站不提供头像存储服务，而是使用 Gravatar 进行头像显示。请使用邮箱注册 WordPress.com，登录 Gravatar 并上传头像。同样使用 Gravatar 的 OJ 有 Vijos、COGS、UOJ 等。

个性签名可以使用 Markdown 与 HTML，请不要在其中添加恶意代码。

添加题目
题面
在添加题目页面填写题面，题目内容使用 Markdown 与 TeX 公式。

测试数据
旧版的 data_rule.txt 已经停止使用，新加入的题目请使用新的 data.yml 配置文件。关于 YAML，详见 YAML 的 Wikipedia 词条。

对于传统和提交答案题目，data.yml 是可选的，系统会自动识别您的数据包，将每个 .in 文件对应到 .out 或 .ans 文件，并将 spj_ 开头的文件作为 Special Judge（详见下文的「Special Judge」）。

传统
对于传统题目，data.yml 的格式如下：

subtasks:
  - score: 30              # 这个子任务的分数（注意，所有子任务的总分必须为 100）
    type: sum              # 子任务类型，可选的值有 sum、min 和 mul.
    cases: [1, 2, 3]       # 测试点编号可为数字
  - score: 30              # 另一个子任务
    type: mul
    cases: ['4', '5', '6'] # 测试点编号也可为字符串

inputFile: 'dat#.in'   # 测试数据包中的输入文件
outputFile: 'dat#.ans' # 测试数据包中的输出文件
# 上述文件名中的 # 字符将被替换为测试点编号

# 可选 - Special Judge
specialJudge:
  language: cpp
  fileName: spj.cpp

# 可选 - 附加源文件
# 例如，给选手提供一个 C++ 头文件，选手可以引用它，调用您提供的代码。
# 常用于封装交互库
extraSourceFiles:
  - language: cpp
    files: # 这是一个数组
     - name: itlib_cpp.h     # 数据包中的文件名
       dest: interaction.h   # 目标文件名，在编译时被放置在与选手程序的同一目录下
  - language: c # 给不同的语言提供不同的附加源文件
    files:
      - name: itlib_c.h
        dest: interaction.h
提交答案
对于提交答案题目，inputFile 和 outputFile 可以省略，用 userOutput 表示用户提交的答案文件名。

userOutput: 'dat#.out'
交互
对于交互题目，需要加入以下内容：

# 交互器
interactor:
  language: cpp
  fileName: interactor.cpp
交互器和选手程序同时运行，交互器的标准输入和标准输出连接了选手程序的标准输出和标准输入 —— 交互通过输入输出进行。

交互器运行时，其目录下会有 input 文件，表示该测试点的输入文件。交互器运行结束后，需要将选手得分写入 score.txt 文件中，并将提供给用户的额外信息输出到标准错误输出（stderr）中。

如果您希望实现 NOI 试题风格的交互（选手通过函数调用与交互器交互），请编写一些头文件作为「附加源文件」并封装标准输入输出的交互。

子任务
子任务是一组测试点的集合，一个子任务的分数由其包含的测试点的分数计算而来，具体的计算方式根据子任务类型而定：

sum：测试点分数按百分比相加；
mul：测试点分数按百分比相乘；
min：取各测试点最低分。
Special Judge
Special Judge 用于传统或提交答案题目。

如果您没有编写 data.yml，请在数据包中添加 spj_LANG.xxx，其中 xxx 为任意后缀名，LANG 为所用语言的简称。

Special Judge 程序运行时，其目录下会有四个文件 input、user_out、answer、code，分别对应该测试点的输入文件、用户输出、该测试点的输出文件、用户的代码（对于非提交答案题目）。

Special Judge 程序运行完成后，应将该测试点的得分输出到标准输出（stdout）中（范围为 0 到 100，将自动折合为测试点分数），并将提供给用户的额外信息输出到标准错误输出（stderr）中。

语言简称
配置数据时，描述 Special Judge 或交互器的语言时使用语言的简称，它们是 c、cpp、cpp11、cpp17、cpp11-clang、cpp17-clang、csharp、haskell、java、nodejs、pascal、python2、python3、ruby。