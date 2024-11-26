import evenmedian

def test_evenmedian():
    # median of [1,3,5]
    assert evenmedian.evenmedian([1, 2, 3, 4, 5]) == 3
    # empty slice
    assert evenmedian.evenmedian([]) == 0
    # single element
    assert evenmedian.evenmedian([42]) == 42
    # median of [2,4]
    assert evenmedian.evenmedian([2, 9, 4]) == 3
    # median of [1,5,9]
    assert evenmedian.evenmedian([1, 0, 5, 0, 9]) == 5
