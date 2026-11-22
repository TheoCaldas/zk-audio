## gain_k2 -> atten_m1 -> dist_d100  (bd 8, B 100)

| audio | N | folds | B total | A total | saving | B fold | A fold | B compress | A compress |
|---|---|---|---|---|---|---|---|---|---|
| 0.11s | 9300 | 3 | 85.6s | 47.7s | **44.3%** | 3.8s | 2.3s | 52.3s | 29.5s |
| 0.53s | 46500 | 15 | 99.8s | 56.7s | **43.2%** | 16.6s | 10.8s | 50.0s | 29.1s |
| 1.02s | 89900 | 29 | 114.8s | 67.7s | **41.0%** | 32.7s | 20.6s | 52.4s | 29.0s |
| 2.00s | 176700 | 57 | 152.8s | 94.0s | **38.5%** | 62.0s | 40.3s | 59.3s | 36.6s |
| 5.03s | 443300 | 143 | 249.6s | 151.4s | **39.3%** | 155.1s | 101.2s | 56.8s | 32.1s |
| 10.02s | 883500 | 285 | 410.2s | 254.4s | **38.0%** | 313.0s | 201.8s | 62.5s | 34.7s |
| 20.03s | 1767000 | 570 | 699.5s | 458.4s | **34.5%** | 608.7s | 407.7s | 57.1s | 32.1s |
| 30.02s | 2647400 | 854 | 989.5s | 648.6s | **34.4%** | 904.3s | 600.8s | 54.3s | 30.2s |

Constraints/step: chain 595,515 (sum of 3 stages), fused 434,105 - 27.1%.

chain  total_prove_ms ~ 90.3s + 1063.3 ms/fold
fused  total_prove_ms ~ 49.3s + 707.9 ms/fold

Fused pads to 2^19 = 524,288 (17% slack).

WARNING chain: compression ms spans 50024-62481 (25%) although the circuit is fixed; worst at 285 folds. Treat absolute times from the affected points as throttled, not as a size effect.
WARNING fused: compression ms spans 29043-36571 (26%) although the circuit is fixed; worst at 57 folds. Treat absolute times from the affected points as throttled, not as a size effect.

Least-squares slope ratio: 33.4%. Fitted fixed-cost saving at 1 fold: 45.2%.
Measured at the largest case (854 folds): folding saving 33.6%, and folding is 91% of the chain's total - so the asymptotic total saving is ~34%.

## gain_k2 -> tremolo_f5  (bd 8, B 100)

| audio | N | folds | B total | A total | saving | B fold | A fold | B compress | A compress |
|---|---|---|---|---|---|---|---|---|---|
| 0.11s | 9300 | 3 | 53.2s | 46.6s | **12.5%** | 2.5s | 1.8s | 31.9s | 27.6s |
| 0.53s | 46500 | 15 | 59.9s | 55.5s | **7.4%** | 10.9s | 10.4s | 30.5s | 28.7s |
| 1.02s | 89900 | 29 | 69.1s | 63.4s | **8.3%** | 20.5s | 15.1s | 31.2s | 29.6s |
| 2.00s | 176700 | 57 | 87.4s | 80.6s | **7.7%** | 39.1s | 29.7s | 29.6s | 33.7s |
| 5.03s | 443300 | 143 | 159.3s | 127.3s | **20.1%** | 107.6s | 74.9s | 32.8s | 35.9s |
| 10.02s | 883500 | 285 | 257.5s | 197.7s | **23.2%** | 208.4s | 151.5s | 31.5s | 29.6s |
| 20.03s | 1767000 | 570 | 449.8s | 339.1s | **24.6%** | 401.9s | 295.6s | 30.7s | 27.7s |
| 30.02s | 2647400 | 854 | 654.3s | 490.9s | **25.0%** | 604.7s | 445.6s | 30.1s | 28.5s |

Constraints/step: chain 401,512 (sum of 2 stages), fused 320,807 - 20.1%.

chain  total_prove_ms ~ 51.2s + 706.2 ms/fold
fused  total_prove_ms ~ 49.0s + 516.0 ms/fold

Fused pads to 2^19 = 524,288 (39% slack).

WARNING fused: compression ms spans 27627-35886 (30%) although the circuit is fixed; worst at 143 folds. Treat absolute times from the affected points as throttled, not as a size effect.

Least-squares slope ratio: 26.9%. Fitted fixed-cost saving at 1 fold: 4.6%.
Measured at the largest case (854 folds): folding saving 26.3%, and folding is 92% of the chain's total - so the asymptotic total saving is ~26%.

