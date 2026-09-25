"""Regenerate with: uv run --with onnx python tests/generate_tensor_fixtures.py."""
from pathlib import Path
import onnx
from onnx import TensorProto as T, helper as h


def save(name, nodes, inputs, outputs):
    model = h.make_model(h.make_graph(nodes, name, inputs, outputs),
                         opset_imports=[h.make_opsetid('', 17)], ir_version=9)
    onnx.checker.check_model(model)
    onnx.save(model, Path(__file__).parent / 'data' / (name + '.onnx'))


for dtype, suffix in [(T.FLOAT, 'fp32'), (T.FLOAT16, 'fp16')]:
    for rank in [0, 1, 2, 3, 5]:
        shape = [2] * rank
        save(f'identity-rank{rank}-{suffix}',
             [h.make_node('Identity', ['input'], ['output'])],
             [h.make_tensor_value_info('input', dtype, shape)],
             [h.make_tensor_value_info('output', dtype, shape)])

save('tokens', [h.make_node('Sub', ['input_ids', 'attention_mask'], ['difference']),
                h.make_node('Cast', ['difference'], ['embeddings'], to=T.FLOAT),
                h.make_node('Cast', ['input_ids'], ['half_embeddings'], to=T.FLOAT16)],
     [h.make_tensor_value_info(n, T.INT64, ['batch', 3])
      for n in ['input_ids', 'attention_mask']],
     [h.make_tensor_value_info('embeddings', T.FLOAT, ['batch', 3]),
      h.make_tensor_value_info('half_embeddings', T.FLOAT16, ['batch', 3])])
