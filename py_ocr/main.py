from io import StringIO
import time

from rapidocr import RapidOCR
import sys
import io

def get_text_and_rect():
    engine = RapidOCR(config_path="rapidocr_config.yaml")
    img_url = sys.argv[1]
    result = engine(img_url)

    buffer = StringIO()
    buffer.write("[")

    rlen = len(result.txts)
    for i in range(rlen):
        key = result.txts[i]
        (lt, lr, rb, lb) = result.boxes[i]
        width = lr[0] - lt[0]
        height = rb[1] - lr[1]
        buffer.write('{\"text\":\"%s\",\"x\":%d,\"y\":%d,\"width\":%d,\"height\":%d' % (key, lt[0], lt[1], width, height))
        if i != rlen - 1:
            buffer.write("},")
        else:
            buffer.write("}")
    buffer.write("]")
    sys.stdout.write(buffer.getvalue())
    sys.stdout.flush()

def get_text():
    engine = RapidOCR(config_path="rapidocr_config.yaml")
    img_url = sys.argv[1]
    result = engine(img_url)

    buffer = StringIO()
    buffer.write("[")

    rlen = len(result.txts)
    for i in range(rlen):
        key = result.txts[i]
        buffer.write('\"%s\"' % (key))
        if i != rlen - 1:
            buffer.write(",")
            
    buffer.write("]")
    sys.stdout.write(buffer.getvalue())
    sys.stdout.flush()


if __name__ == "__main__":
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
    get_text()


    # start = time.perf_counter()
    # main()
    # end = time.perf_counter()
    # print(f"执行耗时: {end - start:.4f} 秒")



